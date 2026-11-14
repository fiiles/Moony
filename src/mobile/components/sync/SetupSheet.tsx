import { useEffect, useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { RecoveryKit } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useDeviceSyncAccount, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useServerChoice } from '@/hooks/use-server-choice';
import { translateApiError } from '@/lib/translate-api-error';
import { accountQueryEnabled, hostOf, setupSteps, type SetupStep } from '@/utils/sync-setup';
import { Sheet } from '@/mobile/components/Sheet';
import { StepProgress } from '@/mobile/components/StepProgress';
import { KitBody } from '@/mobile/components/sync/KitBody';
import { ScanOverlay } from '@/mobile/components/sync/ScanOverlay';
import { InsecureBlock, ServerChoiceBlock } from '@/mobile/components/sync/ServerChoiceBlock';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';
import { scanQrCode } from '@/mobile/lib/qr-scanner';
import { phoneDeviceKey, setupScanRoute } from '@/mobile/lib/sync-scan';
import { field48 } from '@/mobile/pages/first-run/styles';

/** The phone sets up on an own server only: the cloud card is not open yet, and the phone never pays. */
const STEPS = setupSteps('own');

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

interface SetupSheetProps {
  /** Mounted only while a setup runs: closing unmounts it, which also forgets the kit. */
  onClose: () => void;
}

/**
 * Settings → Sync → "Set up sync on this phone" (the first device), the phone's counterpart of the
 * desktop's `SyncSetupDialog`: the connection code of the person's own sync server (typed, pasted or
 * scanned with the camera; the server is asked what it is before anything is sent), the Sync
 * Recovery Kit (copy it, or on iOS save it as text or a page; "I saved my Sync Recovery Kit"
 * unlocks after one of them), then this device's name. A setup that was started and closed is
 * resumed with the stored server; one on plain http asks for the acknowledgement again.
 */
export function SetupSheet({ onClose }: SetupSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const fieldId = useId();
  const { data: status } = useDeviceSyncStatus();
  const [step, setStep] = useState<SetupStep>(STEPS[0]);
  const accountQuery = useDeviceSyncAccount({ enabled: accountQueryEnabled(step, 'own') });
  const { connectOwnServer, prepareRecoveryKit, prepareKitWithPassword, finishSetup } =
    useDeviceSyncMutations();

  const choice = useServerChoice('connectionCode', 'setup');
  /** Setup was started with this own server before (the sheet was closed): continue with it. */
  const resumeAddress =
    status?.phase === 'settingUp' && status.ownServer ? status.serverAddress : null;
  const [otherServer, setOtherServer] = useState(false);
  const resuming = resumeAddress !== null && !otherServer;
  /** The resumed server is plain http: the warning is shown and acknowledged again. */
  const resumeInsecure = resuming && status?.serverInsecure === true;
  /** Why the server step could not go on (a revoked code, a blocked account), translated. */
  const [connectError, setConnectError] = useState<string | null>(null);
  /** The camera is open (the sheet steps aside, as the scan flow's sheets do). */
  const [scanning, setScanning] = useState(false);
  const [createdNumber, setCreatedNumber] = useState<string | null>(null);
  const [kit, setKit] = useState<RecoveryKit | null>(null);
  /** The person closed the password question: the kit step waits for "Show the kit". */
  const [kitPromptClosed, setKitPromptClosed] = useState(false);
  /** The kit left the screen (expired, copy window over): the question says so. */
  const [kitAskedAgain, setKitAskedAgain] = useState(false);
  const [kitKept, setKitKept] = useState(false);
  const [kitConfirmed, setKitConfirmed] = useState(false);
  const [deviceName, setDeviceName] = useState(() =>
    t(`setup.defaultName.${phoneDeviceKey(import.meta.env.TAURI_ENV_PLATFORM ?? '')}`)
  );

  const toastError = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  // Step 2 asks the device password before it shows the kit (audit M11), every time the kit has
  // left the screen.
  const askingForKit = step === 'kit' && !kit && !kitPromptClosed;
  // Continuing forgets the kit (state and mutation result): nothing later in setup reads it, the
  // device step needs only `kitKept`. No re-ask, unlike `dropKit`.
  const leaveKitStep = () => {
    prepareRecoveryKit.reset();
    setKit(null);
    setStep('device');
  };
  const dropKit = () => {
    prepareRecoveryKit.reset();
    setKit(null);
    setKitPromptClosed(false);
    setKitAskedAgain(true);
  };

  // The kit holds the vault key: when the sheet closes it leaves the mutation's result too.
  const resetPreparedKit = prepareRecoveryKit.reset;
  useEffect(() => resetPreparedKit, [resetPreparedKit]);
  // The connection code leaves the mutation's state with the sheet too.
  const resetConnection = connectOwnServer.reset;
  useEffect(() => resetConnection, [resetConnection]);

  /** A different account than the one shown: whatever was prepared for the old one is void. */
  const forgetKit = () => {
    prepareRecoveryKit.reset();
    setKit(null);
    setKitPromptClosed(false);
    setKitAskedAgain(false);
    setKitKept(false);
    setKitConfirmed(false);
  };

  const handleServer = async () => {
    setConnectError(null);
    try {
      if (resuming) {
        if (resumeInsecure && !choice.acknowledged) return;
        // A server that does not answer is its own message; only "not active" is "blocked".
        const { data: current } = await accountQuery.refetch({ throwOnError: true });
        if (!current?.active) {
          setConnectError(tc('validation.syncAccountBlocked'));
          return;
        }
      } else {
        const connected = await connectOwnServer.mutateAsync(choice.input.trim());
        if (connected.accountNumber !== createdNumber) forgetKit();
        setCreatedNumber(connected.accountNumber);
        // The code carries the account: it does not stay in the field once it was used
        choice.setInput('');
      }
      setStep('kit');
    } catch (error) {
      setConnectError(translateApiError(error as Error, tc));
    }
  };

  /** The camera reads a connection code into the field; any other QR is named and refused. */
  const scanCode = async () => {
    setScanning(true);
    try {
      const outcome = await scanQrCode();
      if (outcome.kind === 'denied') {
        toast.error(t('phone.cameraDenied'));
      } else if (outcome.kind === 'code') {
        switch (setupScanRoute(outcome.text)) {
          case 'connection':
            setConnectError(null);
            choice.setInput(outcome.text.trim());
            break;
          case 'pair':
            toast.error(t('codes.pairNotForSetup'));
            break;
          case 'recover':
            toast.error(t('codes.kitNotForSetup'));
            break;
          default:
            toast.error(t('phone.notAConnectionCode'));
        }
      }
    } catch {
      toast.error(t('phone.cameraUnavailable'));
    } finally {
      setScanning(false);
    }
  };

  const handleFinish = async () => {
    try {
      await finishSetup.mutateAsync({ deviceName: deviceName.trim(), kitSaved: kitKept });
      toast(t('setup.device.success'));
      onClose();
    } catch (error) {
      toastError(error);
    }
  };

  const busy = connectOwnServer.isPending || finishSetup.isPending;

  let primary: {
    label: string;
    onClick: () => void;
    disabled?: boolean;
    loading?: boolean;
  };
  switch (step) {
    case 'kit':
      primary = {
        label: t('setup.account.continue'),
        onClick: leaveKitStep,
        disabled: !kitConfirmed,
      };
      break;
    case 'device':
      primary = {
        label: t('setup.device.turnOn'),
        onClick: () => void handleFinish(),
        loading: finishSetup.isPending,
        disabled: deviceName.trim() === '',
      };
      break;
    default:
      primary = {
        label: t('setup.account.continue'),
        onClick: () => void handleServer(),
        loading: connectOwnServer.isPending || (resuming && accountQuery.isFetching),
        disabled: resuming ? resumeInsecure && !choice.acknowledged : !choice.ready,
      };
  }

  const stepNumber = STEPS.indexOf(step) + 1;
  const description =
    step === 'server'
      ? t('setup.server.title')
      : step === 'kit'
        ? t('setup.kit.description')
        : t('setup.device.description');

  return (
    <>
      <Sheet
        open={!scanning}
        full
        onOpenChange={(open) => {
          if (!open && !busy) onClose();
        }}
        title={t('setup.title')}
        description={description}
        footer={
          <>
            <Button
              variant="ghost"
              className={`${BUTTON} px-[18px]`}
              onClick={onClose}
              disabled={busy}
            >
              {t('setup.cancel')}
            </Button>
            <Button
              className={`${BUTTON} flex-1`}
              onClick={primary.onClick}
              disabled={primary.disabled}
              loading={primary.loading}
            >
              {primary.label}
            </Button>
          </>
        }
      >
        <StepProgress
          index={stepNumber}
          total={STEPS.length}
          label={t('setup.step', { current: stepNumber, total: STEPS.length })}
        />
        <div className="space-y-3.5">
          {step === 'server' && (
            <>
              {resuming ? (
                <div className="space-y-2">
                  <p className="m-0 text-m-lead text-ink-2">
                    {t('setup.server.resume', { host: hostOf(resumeAddress ?? '') })}
                  </p>
                  <Button
                    variant="ghost"
                    className="h-auto min-h-11 px-0 text-m-lead font-semibold text-ink-2"
                    onClick={() => {
                      setOtherServer(true);
                      choice.setAcknowledged(false);
                      setConnectError(null);
                    }}
                  >
                    {t('setup.server.other')}
                  </Button>
                  {resumeInsecure && (
                    <InsecureBlock
                      acknowledged={choice.acknowledged}
                      onChange={choice.setAcknowledged}
                    />
                  )}
                </div>
              ) : (
                <ServerChoiceBlock state={choice} showCloud onScan={() => void scanCode()} />
              )}
              {connectError && (
                <p role="alert" className="m-0 text-m-sub text-loss">
                  {connectError}
                </p>
              )}
            </>
          )}

          {step === 'kit' &&
            (kit ? (
              <>
                <KitBody
                  kit={kit}
                  onKept={() => setKitKept(true)}
                  onPasswordRequired={dropKit}
                  onExpired={dropKit}
                />
                <div className="flex min-h-11 items-center gap-3">
                  <Checkbox
                    id={`${fieldId}-confirmed`}
                    checked={kitConfirmed}
                    disabled={!kitKept}
                    onCheckedChange={(checked) => setKitConfirmed(checked === true)}
                  />
                  <Label htmlFor={`${fieldId}-confirmed`} className="text-m-row">
                    {t('setup.kit.confirm')}
                  </Label>
                </div>
                {!kitKept && (
                  <p className="m-0 text-m-sub text-ink-4">{t('setup.kit.saveFirst')}</p>
                )}
              </>
            ) : (
              <div className="space-y-2.5">
                <p className="m-0 text-m-lead text-ink-3">
                  {kitAskedAgain ? t('showKit.again') : t('showKit.description')}
                </p>
                <Button
                  variant="secondary"
                  className={`${BUTTON} w-full`}
                  onClick={() => setKitPromptClosed(false)}
                >
                  {t('showKit.confirm')}
                </Button>
              </div>
            ))}

          {step === 'device' && (
            <div className="space-y-1.5">
              <Label htmlFor={`${fieldId}-name`}>{t('setup.device.name')}</Label>
              <Input
                id={`${fieldId}-name`}
                value={deviceName}
                maxLength={60}
                onChange={(event) => setDeviceName(event.target.value)}
                autoComplete="off"
                enterKeyHint="done"
                className={field48}
              />
            </div>
          )}
        </div>
      </Sheet>
      {scanning && <ScanOverlay hint={t('phone.scanConnectionHint')} />}
      <SyncPasswordSheet
        open={askingForKit}
        onOpenChange={(open) => {
          if (!open) setKitPromptClosed(true);
        }}
        title={t('showKit.title')}
        description={kitAskedAgain ? t('showKit.again') : t('showKit.description')}
        confirmLabel={t('showKit.confirm')}
        closeOnSuccess={false}
        onSubmit={async (password) => {
          setKit(await prepareKitWithPassword(password));
          setKitAskedAgain(false);
        }}
      />
    </>
  );
}
