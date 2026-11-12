import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Check, ChevronRight, Copy } from 'lucide-react';
import type { RecoveryKit, SyncPlan } from '@shared/schema';
import { useDeviceSyncAccount, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { RecoveryKitPanel } from '@/components/settings/sync/RecoveryKitPanel';
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
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Segmented } from '@/components/ui/segmented';
import { groupAccountNumber } from '@/utils/recovery-kit';
import { defaultDeviceKey } from '@/utils/sync-devices';

type Step = 'account' | 'pay' | 'kit' | 'device';
const STEPS: readonly Step[] = ['account', 'pay', 'kit', 'device'];

interface SyncSetupDialogProps {
  /** Mounted only while open: closing unmounts it, which also forgets the kit. */
  onClose: () => void;
}

/** First-device setup: account, payment, the Sync Recovery Kit, then this device's name. */
export function SyncSetupDialog({ onClose }: SyncSetupDialogProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { data: status } = useDeviceSyncStatus();
  const [chosenStep, setStep] = useState<Step>('account');
  const { data: account } = useDeviceSyncAccount({ pollWhileInactive: chosenStep === 'pay' });
  // The payment (or the operator's allow-list) is waited for on step 2; it moves on by itself.
  const step: Step = chosenStep === 'pay' && account?.active ? 'kit' : chosenStep;
  const {
    createAccount,
    startCheckout,
    prepareRecoveryKit,
    prepareKitWithPassword,
    finishSetup,
    devGrant,
  } = useDeviceSyncMutations();

  const [relayInput, setRelayInput] = useState('');
  /** The address the current account was created with; `null` while a resumed one is unknown. */
  const [createdWith, setCreatedWith] = useState<string | null>(null);
  const [createdNumber, setCreatedNumber] = useState<string | null>(null);
  const [relayError, setRelayError] = useState<string | null>(null);
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
  const relayValue = relayInput.trim();
  const hasAccount = accountNumber !== null;
  const accountCurrent =
    hasAccount && (createdWith === null ? relayValue === '' : createdWith === relayValue);

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

  const handleAccount = async () => {
    if (accountCurrent) {
      setStep('pay');
      return;
    }
    setRelayError(null);
    try {
      const created = await createAccount.mutateAsync(relayValue === '' ? undefined : relayValue);
      // A different server means a different account: whatever was prepared for the old one is void.
      setCreatedNumber(created.accountNumber);
      setCreatedWith(relayValue);
      prepareRecoveryKit.reset();
      setKit(null);
      setKitPromptClosed(false);
      setKitAskedAgain(false);
      setKitKept(false);
      setKitConfirmed(false);
      setSelfHosted(false);
    } catch (error) {
      setRelayError(translateApiError(error as Error, tc));
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

  const handleGrant = async () => {
    try {
      await devGrant.mutateAsync(30);
    } catch (error) {
      toastError(error);
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

  const stepNumber = STEPS.indexOf(step) + 1;
  const busy = createAccount.isPending || finishSetup.isPending;

  let primary: { label: string; onClick: () => void; disabled?: boolean; loading?: boolean };
  switch (step) {
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
              {t('setup.step', { current: stepNumber, total: STEPS.length })}
              {' · '}
              {t(
                step === 'account'
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
            {step === 'account' && (
              <>
                {numberBox}
                {accountNumber && (
                  <p className="m-0 text-micro font-500 text-ink-3">{t('setup.account.explain')}</p>
                )}
                <Collapsible>
                  <CollapsibleTrigger className="group flex items-center gap-1 text-caption font-700 text-ink-2">
                    <ChevronRight className="size-3.5 transition-transform group-data-[state=open]:rotate-90" />
                    {t('setup.advanced.toggle')}
                  </CollapsibleTrigger>
                  <CollapsibleContent className="grid gap-1.5 pt-2.5">
                    <Label htmlFor="sync-relay-url">{t('setup.advanced.label')}</Label>
                    <Input
                      id="sync-relay-url"
                      value={relayInput}
                      onChange={(event) => {
                        setRelayInput(event.target.value);
                        setRelayError(null);
                      }}
                      placeholder={t('setup.advanced.placeholder')}
                      aria-invalid={relayError ? true : undefined}
                      autoCapitalize="off"
                      autoCorrect="off"
                      spellCheck={false}
                    />
                    {relayError ? (
                      <p className="m-0 text-micro font-500 text-loss" role="alert">
                        {relayError}
                      </p>
                    ) : (
                      <p className="m-0 text-micro font-500 text-ink-4">
                        {t('setup.advanced.hint')}
                      </p>
                    )}
                  </CollapsibleContent>
                </Collapsible>
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
                    <p className="m-0 text-body text-ink-2">{t('setup.pay.selfHosted')}</p>
                    <p className="m-0 text-micro font-500 text-ink-3">
                      {t('setup.pay.selfHostedWaiting')}
                    </p>
                  </>
                ) : (
                  startCheckout.isSuccess && (
                    <p className="m-0 text-micro font-500 text-ink-3">{t('setup.pay.waiting')}</p>
                  )
                )}
                {status?.devTools && (
                  <div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={handleGrant}
                      loading={devGrant.isPending}
                    >
                      {t('setup.pay.grantTest')}
                    </Button>
                  </div>
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
