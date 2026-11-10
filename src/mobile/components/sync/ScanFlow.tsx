import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDevicePairing } from '@/hooks/use-device-pairing';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { backupFileName } from '@/utils/sync-status';
import { Button } from '@/components/ui/button';
import { Sheet } from '@/mobile/components/Sheet';
import { CodeEntrySheet } from '@/mobile/components/sync/CodeEntrySheet';
import { JoinDetailsSheet, type ScanDetails } from '@/mobile/components/sync/JoinDetailsSheet';
import { PairingSheet, type JoinFailure } from '@/mobile/components/sync/PairingSheet';
import { ScanOverlay } from '@/mobile/components/sync/ScanOverlay';
import { cancelQrScan, openCameraSettings, scanQrCode } from '@/mobile/lib/qr-scanner';
import { scanRoute, type ScanMode } from '@/mobile/lib/sync-scan';

interface ScanFlowProps {
  /** `join`: sync is off ("Scan a code from another device"); `add`: sync is on ("Add a device"). */
  mode: ScanMode;
  /** Developer tools only: the pasted code takes the camera's place from the start. */
  paste: boolean;
  /** The developer tools are on (`status.devTools`): the scanner offers the paste sheet too. */
  devTools: boolean;
  /** The flow is over (finished, cancelled or failed): the caller unmounts it. */
  onDone: () => void;
}

/** Where the flow is: the details, the camera, its refusal, the paste sheet, a request, the pairing. */
type Step = 'details' | 'scanning' | 'denied' | 'paste' | 'working' | 'pairing';

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * Scanning on the phone, from the details to the end (spec §11.4): the password (and when joining,
 * the warning and the device name) before the camera (review PF1), then the camera, then by what was
 * scanned — a pairing code pairs (`scan`, the backend tells the role: joining with sync off, sending
 * the sync details with sync on), the Recovery Kit's QR joins with the Kit (sync off) or is "Sync
 * is already on". Mounted only while it runs; unmounting it (a lock, leaving the page) cancels the
 * camera and gives the pairing up (`useDevicePairing`).
 */
export function ScanFlow({ mode, paste, devTools, onDone }: ScanFlowProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const pairing = useDevicePairing();
  const { joinWithRecoveryKit } = useDeviceSyncMutations();
  const [step, setStep] = useState<Step>('details');
  const [detailsError, setDetailsError] = useState<string | null>(null);
  /** The details between "Continue" and the request that uses them; never rendered. */
  const details = useRef<ScanDetails | null>(null);
  /** The overlay's "Paste a pairing code": the scan is cancelled to make way for the sheet. */
  const toPaste = useRef(false);
  const [joinFailure, setJoinFailure] = useState<JoinFailure | null>(null);

  // A Kit join's password is a mutation variable: it leaves with the flow
  const resetKitJoin = joinWithRecoveryKit.reset;
  useEffect(
    () => () => {
      details.current = null;
      resetKitJoin();
    },
    [resetKitJoin]
  );

  // A successful join clears the pairing before the flow ends: keep showing its last step
  const lastView = useRef(pairing.view);
  if (pairing.view) lastView.current = pairing.view;
  const view = pairing.view ?? lastView.current;

  const fail = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  /** Back to the details with what went wrong; the password is typed again. */
  const backToDetails = (error: unknown) => {
    details.current = null;
    setDetailsError(translateApiError(error as Error, tc));
    setStep('details');
  };

  const giveUp = async () => {
    try {
      await pairing.cancel();
    } catch {
      // Gone on this device either way.
    }
    onDone();
  };

  const takeText = async (text: string) => {
    const typed = details.current;
    if (!typed) return onDone();
    switch (scanRoute(text, mode)) {
      case 'pair':
        setStep('working');
        try {
          await pairing.scan.mutateAsync({
            code: text.trim(),
            password: typed.password,
            deviceName: typed.deviceName,
          });
          details.current = null;
          setStep('pairing');
        } catch (error) {
          pairing.scan.reset();
          backToDetails(error);
        }
        return;
      case 'kitJoin':
        // The busy cover is up while the data is replaced; nothing of the flow is open under it
        setStep('working');
        try {
          const path = await joinWithRecoveryKit.mutateAsync({
            code: text.trim(),
            deviceName: typed.deviceName ?? '',
            password: typed.password,
          });
          details.current = null;
          toast(t('pairing.joined', { path: backupFileName(path) }));
          onDone();
        } catch (error) {
          joinWithRecoveryKit.reset();
          backToDetails(error);
        }
        return;
      case 'alreadyOn':
        details.current = null;
        toast.error(tc('validation.syncAlreadyEnabled'));
        onDone();
        return;
      case 'unknown':
        details.current = null;
        toast.error(t('phone.unknownCode'));
        onDone();
        return;
    }
  };

  const scanNow = async () => {
    toPaste.current = false;
    setStep('scanning');
    try {
      const outcome = await scanQrCode();
      if (outcome.kind === 'code') return void (await takeText(outcome.text));
      if (outcome.kind === 'denied') return setStep('denied');
      if (toPaste.current) return setStep('paste');
      details.current = null;
      onDone();
    } catch {
      details.current = null;
      toast.error(t('phone.cameraUnavailable'));
      onDone();
    }
  };

  const submitDetails = (typed: ScanDetails) => {
    details.current = typed;
    setDetailsError(null);
    if (paste) setStep('paste');
    else void scanNow();
  };

  const runJoin = async () => {
    setJoinFailure(null);
    try {
      const path = await pairing.join.mutateAsync();
      toast(t('pairing.joined', { path: backupFileName(path) }));
      onDone();
    } catch (failure) {
      const key = failure instanceof ApiError ? failure.key : undefined;
      setJoinFailure({
        message: translateApiError(failure as Error, tc),
        // Only these two keep the received session; any other failure ended it.
        retryable:
          key === 'validation.syncChangeInProgress' || key === 'validation.syncJoinSetupPending',
      });
    }
  };

  // The sync details arrived (this phone joins): join at once, once ("Try again" after a failure)
  const joining = useRef(false);
  const stage = view?.stage;
  useEffect(() => {
    if (stage !== 'received' || joining.current) return;
    joining.current = true;
    void runJoin();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stage]);

  const handleMatch = async () => {
    try {
      await pairing.confirm.mutateAsync();
    } catch (error) {
      fail(error);
    }
  };

  // The join ended the pairing: back to the details for a fresh one
  const startAgain = async () => {
    try {
      await pairing.cancel();
    } catch {
      // Gone on this device either way.
    }
    lastView.current = undefined;
    joining.current = false;
    setJoinFailure(null);
    setDetailsError(null);
    setStep('details');
  };

  return (
    <>
      <JoinDetailsSheet
        open={step === 'details'}
        onOpenChange={(next) => {
          if (!next) void giveUp();
        }}
        mode={mode}
        error={detailsError}
        onSubmit={submitDetails}
      />
      {step === 'scanning' && (
        <ScanOverlay
          hint={mode === 'add' ? t('phone.addDeviceHint') : t('phone.scanHint')}
          onDevPaste={
            devTools
              ? () => {
                  toPaste.current = true;
                  void cancelQrScan();
                }
              : undefined
          }
        />
      )}
      <Sheet
        open={step === 'denied'}
        onOpenChange={(next) => {
          if (!next) void giveUp();
        }}
        title={t('phone.scanTitle')}
        description={t('phone.cameraDenied')}
        footer={
          <>
            <Button variant="ghost" className={`${BUTTON} px-[18px]`} onClick={() => void giveUp()}>
              {tc('buttons.cancel')}
            </Button>
            <Button
              className={`${BUTTON} flex-1`}
              onClick={() => {
                void openCameraSettings().catch(() => undefined);
                void giveUp();
              }}
            >
              {t('phone.openSettings')}
            </Button>
          </>
        }
      >
        {null}
      </Sheet>
      <CodeEntrySheet
        open={step === 'paste'}
        onOpenChange={(next) => {
          if (!next) void giveUp();
        }}
        onSubmit={(code) => void takeText(code)}
      />
      {/* The scan is announcing this phone's answer (a pairing code only: a Kit join shows the busy cover) */}
      <Sheet
        open={step === 'working' && pairing.scan.isPending}
        onOpenChange={() => undefined}
        title={t('phone.scanTitle')}
        footer={
          <Button variant="ghost" className={`${BUTTON} flex-1`} disabled>
            {tc('buttons.cancel')}
          </Button>
        }
      >
        <p className="m-0 text-m-lead text-ink-3">{t('phone.connecting')}</p>
      </Sheet>
      {view && (
        <PairingSheet
          open={step === 'pairing' && !pairing.join.isPending}
          view={view}
          pollError={pairing.isError ? (pairing.error as Error) : null}
          onMatch={() => void handleMatch()}
          matching={pairing.confirm.isPending}
          onClose={() => void giveUp()}
          joinFailure={joinFailure}
          onRetryJoin={() => void runJoin()}
          onStartAgain={() => void startAgain()}
        />
      )}
    </>
  );
}
