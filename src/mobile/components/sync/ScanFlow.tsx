import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDevicePairing } from '@/hooks/use-device-pairing';
import { useReplaceImpact } from '@/hooks/use-device-sync';
import { useServerChoice } from '@/hooks/use-server-choice';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { joinPresentation } from '@/utils/join-context';
import { consentOf } from '@/utils/replace-impact';
import { IMPACT_CHANGED_KEY } from '@/components/settings/sync/replace-outcome';
import { ReplaceImpactBlock } from '@/mobile/components/sync/ReplaceImpactBlock';
import { translateApiError } from '@/lib/translate-api-error';
import { backupFileName } from '@/utils/sync-status';
import { isDefaultServer } from '@/utils/sync-server';
import { needsJoinAcknowledgement } from '@/utils/sync-setup';
import { Button } from '@/components/ui/button';
import { Sheet } from '@/mobile/components/Sheet';
import { CodeEntrySheet } from '@/mobile/components/sync/CodeEntrySheet';
import { JoinDetailsSheet, type ScanDetails } from '@/mobile/components/sync/JoinDetailsSheet';
import { PairingSheet, type JoinFailure } from '@/mobile/components/sync/PairingSheet';
import { ScanOverlay } from '@/mobile/components/sync/ScanOverlay';
import { ServerCheckBlock } from '@/mobile/components/sync/ServerChoiceBlock';
import { cancelQrScan, openCameraSettings, scanQrCode } from '@/mobile/lib/qr-scanner';
import type { ReplaceOutcomeView } from '@shared/schema';
import { kitQrSummary, scanRoute, type ScanMode } from '@/mobile/lib/sync-scan';

interface ScanFlowProps {
  /** `join`: sync is off ("Scan a code from another device"); `add`: sync is on ("Add a device"). */
  mode: ScanMode;
  /** Developer tools only: the pasted code takes the camera's place from the start. */
  paste: boolean;
  /** The developer tools are on (`status.devTools`): the scanner offers the paste sheet too. */
  devTools: boolean;
  /** The flow is over (finished, cancelled or failed): the caller unmounts it. */
  onDone: () => void;
  /** A join is done: its headline and outcome (the page says where the backup is). */
  onReplaced: (headline: string, outcome: ReplaceOutcomeView) => void;
  /**
   * The first run's Připojení step (spec 2026-11-14 §5.2), `join` only: the step is this flow's
   * details screen, so the flow starts as if the details had been submitted with these (the
   * password typed a step earlier, the name on the step), and every way back to the details goes
   * back to the step. A brand-new profile has nothing to replace: no replace warning, no impact
   * list (the impact is still read: the join sends it as its consent), and the join reads
   * "Připojit".
   */
  firstRun?: { password: string; deviceName: string };
}

/**
 * Where the flow is: the details, the camera, its refusal, a QR that is no Moony code, the paste
 * sheet, the confirm before a Recovery Kit QR joins, a request, the pairing.
 */
type Step =
  'details' | 'scanning' | 'denied' | 'notMoony' | 'paste' | 'kitConfirm' | 'working' | 'pairing';

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * Scanning on the phone, from the details to the end (spec §11.4): the password (and when joining,
 * the warning and the device name) before the camera (review PF1), then the camera, then by what was
 * scanned — a pairing code pairs (`scan`, the backend tells the role: joining with sync off, sending
 * the sync details with sync on), the Recovery Kit's QR joins with the Kit once its account and
 * sync server are confirmed (sync off) or is "Sync is already on". Mounted only while it runs;
 * unmounting it (a lock, leaving the page) cancels the camera and gives the pairing up
 * (`useDevicePairing`).
 */
export function ScanFlow({ mode, paste, devTools, onDone, onReplaced, firstRun }: ScanFlowProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const presentation = joinPresentation(
    firstRun ? { kind: 'firstRun', password: firstRun.password } : { kind: 'settings' }
  );
  const pairing = useDevicePairing();
  const { joinWithRecoveryKit, turnOff } = useDeviceSyncMutations();
  // The first run starts past the details: at the paste sheet, or at the camera (started below)
  const [step, setStep] = useState<Step>(firstRun ? (paste ? 'paste' : 'scanning') : 'details');
  const [detailsError, setDetailsError] = useState<string | null>(null);
  /** The details between "Continue" and the request that uses them; never rendered. */
  const details = useRef<ScanDetails | null>(firstRun ?? null);
  /** A scanned Recovery Kit QR between the scan and "Join" (it holds the recovery key); not rendered. */
  const kitText = useRef<string | null>(null);
  /** Why the scanned QR is no code for this flow (the `notMoony` sheet's sentence). */
  const [refusal, setRefusal] = useState<'unknownCode' | 'connection'>('unknownCode');
  /** What the confirm sheet shows of that QR: the account number and the sync server. */
  const [kitShown, setKitShown] = useState<{ account: string; server: string } | null>(null);
  /** The overlay's "Paste a pairing code": the scan is cancelled to make way for the sheet. */
  const toPaste = useRef(false);
  const [joinFailure, setJoinFailure] = useState<JoinFailure | null>(null);
  /** The warning at `received` for a plain http server (the pairing names it, the phone typed none). */
  const [insecureAcknowledged, setInsecureAcknowledged] = useState(false);
  // A scanned Kit QR naming a server other than the Moony cloud: asked what it is, and for plain
  // http acknowledged, before the join sends the account to it
  const kitServer = useServerChoice(
    'addressOrCode',
    'server',
    kitShown && !isDefaultServer(kitShown.server) ? kitShown.server : ''
  );
  const kitServerChecked = !kitShown || isDefaultServer(kitShown.server) || kitServer.ready;
  /** What the person saw no longer held and was read again (`validation.syncReplaceImpactChanged`). */
  const [impactChanged, setImpactChanged] = useState<string | null>(null);
  /** Cancel was pressed while the scan was announcing: its late answer is given up too. */
  const abandoned = useRef(false);

  // A Kit join's password is a mutation variable: it leaves with the flow
  const resetKitJoin = joinWithRecoveryKit.reset;
  useEffect(
    () => () => {
      details.current = null;
      kitText.current = null;
      resetKitJoin();
    },
    [resetKitJoin]
  );

  // A successful join clears the pairing before the flow ends: keep showing its last step
  const lastView = useRef(pairing.view);
  if (pairing.view) lastView.current = pairing.view;
  const view = pairing.view ?? lastView.current;
  const joinNeedsAcknowledgement = view ? needsJoinAcknowledgement(view, null) : false;

  // What a join puts at stake, read when its confirmation opens (a Kit QR's, or the received
  // sync details'); the join sends the consent of the numbers on screen
  const stage = view?.stage;
  const confirming = step === 'kitConfirm' || (step === 'pairing' && stage === 'received');
  const impact = useReplaceImpact(confirming);
  const shownImpact = impact.isFetching ? undefined : impact.data;
  // The first run lists nothing, but a failed read still says why the join stays off
  const impactBlock =
    presentation.showImpact || impact.isError ? (
      <ReplaceImpactBlock
        flow="join"
        impact={shownImpact}
        error={impact.isError ? (impact.error as Error) : null}
      />
    ) : null;

  const fail = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  /** Every way out goes through here: the password typed for the scan goes with it. */
  const finish = () => {
    details.current = null;
    kitText.current = null;
    onDone();
  };

  /**
   * Back to the details with what went wrong; the password is typed again. In the first run the
   * details are the Připojení step: the flow ends there, and the toast says what went wrong.
   */
  const backToDetails = (error: unknown) => {
    if (firstRun) {
      fail(error);
      finish();
      return;
    }
    details.current = null;
    kitText.current = null;
    setDetailsError(translateApiError(error as Error, tc));
    setStep('details');
  };

  const giveUp = async () => {
    details.current = null;
    kitText.current = null;
    try {
      await pairing.cancel();
    } catch {
      // Gone on this device either way.
    }
    finish();
  };

  // Cancel while "Connecting…": the flow ends now; the scan's answer, when it comes, is cancelled
  // too (the backend's cancel also waits for the scan to finish), so no pairing is left behind.
  const abandonScan = () => {
    abandoned.current = true;
    void giveUp();
  };

  const takeText = async (text: string) => {
    const typed = details.current;
    if (!typed) return finish();
    const route = scanRoute(text, mode);
    switch (route) {
      case 'pair':
        setStep('working');
        try {
          await pairing.scan.mutateAsync({
            code: text.trim(),
            password: typed.password,
            deviceName: typed.deviceName,
          });
          details.current = null;
          if (abandoned.current) {
            void pairing.cancel().catch(() => undefined);
            return;
          }
          setStep('pairing');
        } catch (error) {
          pairing.scan.reset();
          if (abandoned.current) return;
          backToDetails(error);
        }
        return;
      case 'kitJoin': {
        // A QR is read at a glance: say which account and server it joins before the data goes
        const summary = kitQrSummary(text);
        if (!summary.ok && firstRun) {
          toast.error(tc(summary.problem));
          finish();
          return;
        }
        if (!summary.ok) {
          details.current = null;
          setDetailsError(tc(summary.problem));
          setStep('details');
          return;
        }
        kitText.current = text.trim();
        setKitShown({ account: summary.account, server: summary.server });
        setStep('kitConfirm');
        return;
      }
      case 'alreadyOn':
        toast.error(tc('validation.syncAlreadyEnabled'));
        finish();
        return;
      case 'connection':
      case 'unknown':
        // Another QR (or a sync server's connection code, which is for setup): say so and offer the
        // camera again; the password stays for that retry only
        setRefusal(route === 'connection' ? 'connection' : 'unknownCode');
        setStep('notMoony');
        return;
    }
  };

  /** "Join" on the confirm sheet: the scanned Kit QR joins. */
  const joinWithKit = async () => {
    const typed = details.current;
    const code = kitText.current;
    if (!typed || code === null || !shownImpact || !kitServerChecked) return finish();
    // The busy cover is up while the data is replaced; nothing of the flow is open under it
    setStep('working');
    try {
      const outcome = await joinWithRecoveryKit.mutateAsync({
        code,
        deviceName: typed.deviceName ?? '',
        password: typed.password,
        consent: consentOf('join', shownImpact),
      });
      finish();
      onReplaced(t('pairing.joined', { path: backupFileName(outcome.backupPath) }), outcome);
    } catch (error) {
      joinWithRecoveryKit.reset();
      if (error instanceof ApiError && error.key === IMPACT_CHANGED_KEY) {
        // What the confirmation showed no longer holds: the Kit confirmation shows the new numbers
        setStep('kitConfirm');
        setImpactChanged(translateApiError(error, tc));
        await impact.refetch();
        return;
      }
      backToDetails(error);
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
      finish();
    } catch {
      toast.error(t('phone.cameraUnavailable'));
      finish();
    }
  };

  const submitDetails = (typed: ScanDetails) => {
    details.current = typed;
    setDetailsError(null);
    if (paste) setStep('paste');
    else void scanNow();
  };

  // The first run: the step's details stand in for the details sheet, once, as the flow mounts
  const started = useRef(false);
  useEffect(() => {
    if (!firstRun || started.current) return;
    started.current = true;
    submitDetails(firstRun);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once per mount, with the details it mounted with
  }, []);

  const runJoin = async () => {
    setJoinFailure(null);
    setImpactChanged(null);
    // The consent is that of the numbers on screen; the join waits for the person's confirmation
    if (!shownImpact) return;
    try {
      const outcome = await pairing.join.mutateAsync(consentOf('join', shownImpact));
      finish();
      onReplaced(t('pairing.joined', { path: backupFileName(outcome.backupPath) }), outcome);
    } catch (failure) {
      if (failure instanceof ApiError && failure.key === IMPACT_CHANGED_KEY) {
        // Shown again with the new numbers, and the person asked again
        setImpactChanged(translateApiError(failure, tc));
        await impact.refetch();
        return;
      }
      // The key alone cannot tell whether the backend kept the received session: ask it.
      const action = await pairing.afterFailedJoin(failure);
      setJoinFailure({ message: translateApiError(failure as Error, tc), ...action });
    }
  };

  // The holder's typed digits, or the joiner's Continue: a failure shows in the sheet
  // (`confirmError`), and one that ended the pairing is `pairing.error`.
  const handleConfirm = (typedCode?: string) => {
    pairing.confirm.mutate(typedCode);
  };

  // A half-finished setup on this phone refuses the join: cancel it, then join again
  const cancelSetupAndRetry = async () => {
    try {
      await turnOff.mutateAsync();
    } catch (error) {
      fail(error);
      return;
    }
    void runJoin();
  };

  // The join ended the pairing: back to the details for a fresh one (the first run's are the step)
  const startAgain = async () => {
    if (firstRun) return giveUp();
    try {
      await pairing.cancel();
    } catch {
      // Gone on this device either way.
    }
    lastView.current = undefined;
    setJoinFailure(null);
    setInsecureAcknowledged(false);
    setImpactChanged(null);
    setDetailsError(null);
    setStep('details');
  };

  return (
    <>
      {!firstRun && (
        <JoinDetailsSheet
          open={step === 'details'}
          onOpenChange={(next) => {
            if (!next) void giveUp();
          }}
          mode={mode}
          error={detailsError}
          onSubmit={submitDetails}
        />
      )}
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
      <Sheet
        open={step === 'notMoony'}
        onOpenChange={(next) => {
          if (!next) void giveUp();
        }}
        title={t('phone.scanTitle')}
        description={
          refusal === 'connection' ? t('codes.connectionNotHere') : t('phone.unknownCode')
        }
        footer={
          <>
            <Button variant="ghost" className={`${BUTTON} px-[18px]`} onClick={() => void giveUp()}>
              {tc('buttons.cancel')}
            </Button>
            <Button className={`${BUTTON} flex-1`} onClick={() => void scanNow()}>
              {t('pairing.tryAgain')}
            </Button>
          </>
        }
      >
        {null}
      </Sheet>
      <Sheet
        open={step === 'kitConfirm'}
        onOpenChange={(next) => {
          if (!next) void giveUp();
        }}
        title={t('phone.kitConfirmTitle')}
        description={presentation.showReplaceWarning ? t('phone.kitConfirmBody') : undefined}
        footer={
          <>
            <Button variant="ghost" className={`${BUTTON} px-[18px]`} onClick={() => void giveUp()}>
              {tc('buttons.cancel')}
            </Button>
            <Button
              variant={firstRun ? 'default' : 'destructive'}
              className={`${BUTTON} flex-1`}
              disabled={!shownImpact || !kitServerChecked}
              onClick={() => void joinWithKit()}
            >
              {firstRun ? t('join.connect') : t('join.confirm')}
            </Button>
          </>
        }
      >
        {kitShown && (
          <dl className="m-0 mb-3 grid gap-3">
            <div>
              <dt className="text-m-caption text-ink-3">{t('join.accountLabel')}</dt>
              <dd className="m-0 text-m-row text-ink num" data-testid="kit-confirm-account">
                {kitShown.account}
              </dd>
            </div>
            <div>
              <dt className="text-m-caption text-ink-3">{t('join.serverLabel')}</dt>
              <dd className="m-0 break-all text-m-row text-ink" data-testid="kit-confirm-server">
                {kitShown.server}
              </dd>
            </div>
          </dl>
        )}
        {kitShown && !isDefaultServer(kitShown.server) && (
          <div className="mb-3">
            <ServerCheckBlock state={kitServer} />
          </div>
        )}
        {impactChanged && step === 'kitConfirm' && (
          <p role="alert" className="mx-0 mb-3 mt-0 text-m-sub font-medium text-loss">
            {impactChanged}
          </p>
        )}
        {impactBlock}
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
        onOpenChange={(next) => {
          if (!next) abandonScan();
        }}
        title={t('phone.scanTitle')}
        footer={
          <Button variant="ghost" className={`${BUTTON} flex-1`} onClick={abandonScan}>
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
          confirmError={pairing.confirmError}
          onConfirm={handleConfirm}
          confirming={pairing.confirm.isPending}
          onClose={() => void giveUp()}
          joinFailure={joinFailure}
          onRetryJoin={() => void runJoin()}
          onStartAgain={() => void startAgain()}
          onCancelSetup={() => void cancelSetupAndRetry()}
          cancellingSetup={turnOff.isPending}
          impact={impactBlock}
          askInsecure={joinNeedsAcknowledgement}
          insecureAcknowledged={insecureAcknowledged}
          onInsecureAcknowledged={setInsecureAcknowledged}
          canJoin={shownImpact !== undefined && (!joinNeedsAcknowledgement || insecureAcknowledged)}
          onJoin={() => void runJoin()}
          impactChanged={impactChanged}
          firstRun={firstRun !== undefined}
        />
      )}
    </>
  );
}
