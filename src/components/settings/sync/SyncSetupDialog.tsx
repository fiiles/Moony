import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Check, Copy } from 'lucide-react';
import type { RecoveryKit, SyncPlan } from '@shared/schema';
import { useDeviceSyncAccount, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useServerChoice } from '@/hooks/use-server-choice';
import { RecoveryKitPanel } from '@/components/settings/sync/RecoveryKitPanel';
import { InsecureNotice, ServerChoice } from '@/components/settings/sync/ServerChoice';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Segmented } from '@/components/ui/segmented';
import { groupAccountNumber } from '@/utils/recovery-kit';
import { defaultDeviceKey } from '@/utils/sync-devices';
import {
  accountQueryEnabled,
  hostOf,
  setupSteps,
  type ServerKind,
  type SetupStep,
} from '@/utils/sync-setup';

interface SyncSetupDialogProps {
  /** Mounted only while open: closing unmounts it, which also forgets the kit. */
  onClose: () => void;
}

/**
 * First-device setup. On an own server: its connection code, the Sync Recovery Kit, then this
 * device's name. The Moony cloud (not open yet) adds an account and a payment before the kit.
 */
export function SyncSetupDialog({ onClose }: SyncSetupDialogProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { data: status } = useDeviceSyncStatus();
  // The cloud card is disabled for now, so nothing sets this to 'cloud'; its steps stay below.
  const [kind] = useState<ServerKind>('own');
  const steps = setupSteps(kind);
  const [chosenStep, setStep] = useState<SetupStep>(steps[0]);
  const accountQuery = useDeviceSyncAccount({
    pollWhileInactive: chosenStep === 'pay',
    enabled: accountQueryEnabled(chosenStep, kind),
  });
  const account = accountQuery.data;
  // The payment is waited for on the pay step; it moves on by itself.
  const step: SetupStep = chosenStep === 'pay' && account?.active ? 'kit' : chosenStep;
  const {
    createAccount,
    connectOwnServer,
    startCheckout,
    prepareRecoveryKit,
    prepareKitWithPassword,
    finishSetup,
  } = useDeviceSyncMutations();

  const choice = useServerChoice('connectionCode', 'setup');
  /** Setup was started with this own server before (the dialog was closed): continue with it. */
  const resumeAddress =
    status?.phase === 'settingUp' && status.ownServer ? status.serverAddress : null;
  const [otherServer, setOtherServer] = useState(false);
  const resuming = resumeAddress !== null && !otherServer;
  /** The resumed server is plain http: the warning is shown and acknowledged again. */
  const resumeInsecure = resuming && status?.serverInsecure === true;
  /** Why the server step could not go on (a revoked code, a blocked account), translated. */
  const [connectError, setConnectError] = useState<string | null>(null);
  const [createdNumber, setCreatedNumber] = useState<string | null>(null);
  const [plan, setPlan] = useState<SyncPlan>('monthly');
  const [selfHosted, setSelfHosted] = useState(false);
  const [kit, setKit] = useState<RecoveryKit | null>(null);
  /** The person closed the password question: the kit step waits for "Show the kit". */
  const [kitPromptClosed, setKitPromptClosed] = useState(false);
  /** The kit left the screen (expired, copy window over): the question says so. */
  const [kitAskedAgain, setKitAskedAgain] = useState(false);
  const [kitKept, setKitKept] = useState(false);
  const [kitConfirmed, setKitConfirmed] = useState(false);
  const [numberCopied, setNumberCopied] = useState(false);
  const [deviceName, setDeviceName] = useState(() => t(`setup.defaultName.${defaultDeviceKey()}`));

  const accountNumber = createdNumber ?? account?.accountNumber ?? null;
  const accountCurrent = accountNumber !== null;

  const toastError = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  // Step 3 asks the device password before it shows the kit (audit M11), every time the kit has
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

  // The kit holds the vault key: when the dialog closes it leaves the mutation's result too.
  const resetPreparedKit = prepareRecoveryKit.reset;
  useEffect(() => resetPreparedKit, [resetPreparedKit]);
  // The connection code leaves the mutation's state with the dialog too.
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
    if (kind === 'cloud') {
      setStep('account');
      return;
    }
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

  const handleAccount = async () => {
    if (accountCurrent) {
      setStep('pay');
      return;
    }
    try {
      const created = await createAccount.mutateAsync(undefined);
      setCreatedNumber(created.accountNumber);
      forgetKit();
      setSelfHosted(false);
    } catch (error) {
      toastError(error);
    }
  };

  const handleCheckout = async () => {
    try {
      const target = await startCheckout.mutateAsync(plan);
      if (target.kind === 'open') await openUrl(target.url);
      else setSelfHosted(true);
    } catch (error) {
      toastError(error);
    }
  };

  const handleFinish = async () => {
    try {
      await finishSetup.mutateAsync({ deviceName: deviceName.trim() });
      toast(t('setup.device.success'));
      onClose();
    } catch (error) {
      toastError(error);
    }
  };

  const copyNumber = async () => {
    if (!accountNumber) return;
    try {
      await navigator.clipboard.writeText(accountNumber);
      setNumberCopied(true);
      setTimeout(() => setNumberCopied(false), 2000);
    } catch (error) {
      toastError(error);
    }
  };

  const numberBox = accountNumber && (
    <div>
      <Label>{t('setup.account.number')}</Label>
      <div
        className="mt-1.5 select-all rounded-r3 bg-dark px-4 py-[14px] text-center text-[19px] font-650 tracking-[0.08em] text-ink-inverse num"
        data-testid="sync-account-number"
      >
        {groupAccountNumber(accountNumber)}
      </div>
      <div className="mt-2">
        <Button type="button" variant="outline" size="sm" onClick={copyNumber}>
          {numberCopied ? <Check /> : <Copy />}
          {numberCopied ? t('setup.account.copied') : t('setup.account.copy')}
        </Button>
      </div>
    </div>
  );

  const stepNumber = steps.indexOf(step) + 1;
  const busy = createAccount.isPending || connectOwnServer.isPending || finishSetup.isPending;

  let primary: { label: string; onClick: () => void; disabled?: boolean; loading?: boolean };
  switch (step) {
    case 'server':
      primary = {
        label: t('setup.account.continue'),
        onClick: handleServer,
        loading: connectOwnServer.isPending || (resuming && accountQuery.isFetching),
        disabled: resuming ? resumeInsecure && !choice.acknowledged : !choice.ready,
      };
      break;
    case 'account':
      primary = {
        label: accountCurrent ? t('setup.account.continue') : t('setup.account.create'),
        onClick: handleAccount,
        loading: createAccount.isPending,
      };
      break;
    case 'pay':
      primary = {
        label: t('setup.pay.continue'),
        onClick: handleCheckout,
        loading: startCheckout.isPending,
        disabled: selfHosted,
      };
      break;
    case 'kit':
      primary = {
        label: t('setup.account.continue'),
        onClick: leaveKitStep,
        disabled: !kitConfirmed,
      };
      break;
    default:
      primary = {
        label: t('setup.device.turnOn'),
        onClick: handleFinish,
        loading: finishSetup.isPending,
        disabled: deviceName.trim() === '',
      };
  }

  return (
    <>
      <Dialog open onOpenChange={(open) => !open && !busy && onClose()}>
        <DialogContent className="max-w-[560px]">
          <DialogHeader>
            <DialogTitle>{t('setup.title')}</DialogTitle>
            <DialogDescription>
              {t('setup.step', { current: stepNumber, total: steps.length })}
              {' · '}
              {t(
                step === 'server'
                  ? 'setup.server.title'
                  : step === 'account'
                    ? 'setup.account.description'
                    : step === 'pay'
                      ? 'setup.pay.description'
                      : step === 'kit'
                        ? 'setup.kit.description'
                        : 'setup.device.description'
              )}
            </DialogDescription>
          </DialogHeader>

          <div className="grid gap-4">
            {step === 'server' && (
              <>
                {resuming ? (
                  <div className="grid justify-items-start gap-2">
                    <p className="m-0 text-body text-ink-2">
                      {t('setup.server.resume', { host: hostOf(resumeAddress ?? '') })}
                    </p>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => {
                        setOtherServer(true);
                        choice.setAcknowledged(false);
                        setConnectError(null);
                      }}
                    >
                      {t('setup.server.other')}
                    </Button>
                    {resumeInsecure && (
                      <InsecureNotice
                        id="sync-server-resume"
                        acknowledged={choice.acknowledged}
                        onChange={choice.setAcknowledged}
                      />
                    )}
                  </div>
                ) : (
                  <ServerChoice state={choice} id="sync-server" showCloud />
                )}
                {connectError && (
                  <p className="m-0 text-micro font-500 text-loss" role="alert">
                    {connectError}
                  </p>
                )}
              </>
            )}

            {step === 'account' && (
              <>
                {numberBox}
                {accountNumber && (
                  <p className="m-0 text-micro font-500 text-ink-3">{t('setup.account.explain')}</p>
                )}
              </>
            )}

            {step === 'pay' && (
              <>
                {!selfHosted && (
                  <Segmented<SyncPlan>
                    aria-label={t('setup.pay.plan')}
                    fullWidth
                    value={plan}
                    onValueChange={setPlan}
                    options={[
                      { value: 'monthly', label: t('setup.pay.monthly') },
                      { value: 'yearly', label: t('setup.pay.yearly') },
                    ]}
                  />
                )}
                {selfHosted ? (
                  <>
                    {numberBox}
                    <p className="m-0 text-body text-ink-2">{t('setup.pay.ownServer')}</p>
                  </>
                ) : (
                  startCheckout.isSuccess && (
                    <p className="m-0 text-micro font-500 text-ink-3">{t('setup.pay.waiting')}</p>
                  )
                )}
              </>
            )}

            {step === 'kit' &&
              (kit ? (
                <>
                  <RecoveryKitPanel
                    kit={kit}
                    onKept={() => setKitKept(true)}
                    onPasswordRequired={dropKit}
                    onExpired={dropKit}
                  />
                  <div className="flex items-center gap-2">
                    <Checkbox
                      id="sync-kit-confirmed"
                      checked={kitConfirmed}
                      disabled={!kitKept}
                      onCheckedChange={(checked) => setKitConfirmed(checked === true)}
                    />
                    <Label htmlFor="sync-kit-confirmed" className="text-body font-500">
                      {t('setup.kit.confirm')}
                    </Label>
                  </div>
                  {!kitKept && (
                    <p className="m-0 text-micro font-500 text-ink-4">{t('setup.kit.saveFirst')}</p>
                  )}
                </>
              ) : (
                <div className="grid justify-items-start gap-2">
                  <p className="m-0 text-body text-ink-3">
                    {kitAskedAgain ? t('showKit.again') : t('showKit.description')}
                  </p>
                  <Button type="button" variant="outline" onClick={() => setKitPromptClosed(false)}>
                    {t('showKit.confirm')}
                  </Button>
                </div>
              ))}

            {step === 'device' && (
              <div className="grid gap-1.5">
                <Label htmlFor="sync-device-name">{t('setup.device.name')}</Label>
                <Input
                  id="sync-device-name"
                  value={deviceName}
                  maxLength={60}
                  onChange={(event) => setDeviceName(event.target.value)}
                />
              </div>
            )}
          </div>

          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose} disabled={busy}>
              {t('setup.cancel')}
            </Button>
            <Button
              type="button"
              onClick={primary.onClick}
              disabled={primary.disabled}
              loading={primary.loading}
            >
              {primary.label}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {askingForKit && (
        <SyncPasswordDialog
          title={t('showKit.title')}
          description={kitAskedAgain ? t('showKit.again') : t('showKit.description')}
          confirmLabel={t('showKit.confirm')}
          closeOnSuccess={false}
          onSubmit={async (password) => {
            setKit(await prepareKitWithPassword(password));
            setKitAskedAgain(false);
          }}
          onClose={() => setKitPromptClosed(true)}
        />
      )}
    </>
  );
}
