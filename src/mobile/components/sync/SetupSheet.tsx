import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Check, ChevronRight, Copy, Loader2 } from 'lucide-react';
import type { RecoveryKit } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useDeviceSyncAccount, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { groupAccountNumber } from '@/utils/recovery-kit';
import { Sheet } from '@/mobile/components/Sheet';
import { StepProgress } from '@/mobile/components/StepProgress';
import { KitBody } from '@/mobile/components/sync/KitBody';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';
import { phoneDeviceKey } from '@/mobile/lib/sync-scan';
import { field48, fieldMessage } from '@/mobile/pages/first-run/styles';

type Step = 'account' | 'pay' | 'kit' | 'device';
const STEPS: readonly Step[] = ['account', 'pay', 'kit', 'device'];

/**
 * What the payment step knows. The phone never opens a checkout (App Store rule 3.1.3(b), spec §16a
 * Q4): `startCheckout` is asked once only to learn whether the server is the person's own.
 */
type Pay = 'asking' | 'selfHosted' | 'unavailable' | 'failed';

/** A field that takes an address never feeds the keyboard's corrections. */
const PLAIN_FIELD = { autoCapitalize: 'none', autoCorrect: 'off', spellCheck: false } as const;

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

interface SetupSheetProps {
  /** Mounted only while a setup runs: closing unmounts it, which also forgets the kit. */
  onClose: () => void;
}

/**
 * Settings → Sync → "Set up sync on this phone" (the first device), the phone's counterpart of the
 * desktop's `SyncSetupDialog`: account (with the sync server address under Advanced), payment, the
 * Sync Recovery Kit (copy it, or on iOS save it as text or a page; "I saved my Sync Recovery Kit"
 * unlocks after one of them), then this device's name. On Moony's own server the payment step says
 * that paying is not available in the phone app and waits for the account to become active (a
 * payment made elsewhere, or the developer tools' test subscription); on the person's own server it
 * shows the account number and the ALLOWED_ACCOUNTS instruction and waits for the operator.
 */
export function SetupSheet({ onClose }: SetupSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const fieldId = useId();
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
  const [pay, setPay] = useState<Pay>('asking');
  const [kit, setKit] = useState<RecoveryKit | null>(null);
  /** The person closed the password question: the kit step waits for "Show the kit". */
  const [kitPromptClosed, setKitPromptClosed] = useState(false);
  /** The kit left the screen (expired, copy window over): the question says so. */
  const [kitAskedAgain, setKitAskedAgain] = useState(false);
  const [kitKept, setKitKept] = useState(false);
  const [kitConfirmed, setKitConfirmed] = useState(false);
  const [numberCopied, setNumberCopied] = useState(false);
  const copiedTimer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (copiedTimer.current !== null) window.clearTimeout(copiedTimer.current);
    },
    []
  );
  const [deviceName, setDeviceName] = useState(() =>
    t(`setup.defaultName.${phoneDeviceKey(import.meta.env.TAURI_ENV_PLATFORM ?? '')}`)
  );

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
  const dropKit = () => {
    prepareRecoveryKit.reset();
    setKit(null);
    setKitPromptClosed(false);
    setKitAskedAgain(true);
  };

  // The kit holds the vault key: when the sheet closes it leaves the mutation's result too.
  const resetPreparedKit = prepareRecoveryKit.reset;
  useEffect(() => resetPreparedKit, [resetPreparedKit]);

  // Entering step 2 asks the server's kind once; "Try again" asks again after a failure.
  const asking = useRef(false);
  useEffect(() => {
    if (step !== 'pay' || pay !== 'asking' || asking.current) return;
    asking.current = true;
    startCheckout
      .mutateAsync('monthly')
      .then((target) => setPay(target.kind === 'selfHosted' ? 'selfHosted' : 'unavailable'))
      .catch((error) => {
        setPay('failed');
        toastError(error);
      })
      .finally(() => {
        asking.current = false;
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step, pay]);

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
      setPay('asking');
    } catch (error) {
      setRelayError(translateApiError(error as Error, tc));
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
      if (copiedTimer.current !== null) window.clearTimeout(copiedTimer.current);
      copiedTimer.current = window.setTimeout(() => setNumberCopied(false), 2000);
    } catch (error) {
      toastError(error);
    }
  };

  const numberBox = accountNumber && (
    <div className="space-y-1.5">
      <Label>{t('setup.account.number')}</Label>
      <div
        className="select-all rounded-r3 bg-dark px-4 py-3.5 text-center text-m-row tracking-[0.08em] text-ink-inverse num"
        data-testid="sync-account-number"
      >
        {groupAccountNumber(accountNumber)}
      </div>
      <Button
        variant="secondary"
        className={`${BUTTON} w-full [&_svg]:size-[18px]`}
        onClick={() => void copyNumber()}
      >
        {numberCopied ? <Check aria-hidden /> : <Copy aria-hidden />}
        {numberCopied ? t('setup.account.copied') : t('setup.account.copy')}
      </Button>
    </div>
  );

  const busy = createAccount.isPending || finishSetup.isPending;

  let primary: {
    label: string;
    onClick: () => void;
    disabled?: boolean;
    loading?: boolean;
  } | null;
  switch (step) {
    case 'account':
      primary = {
        label: accountCurrent ? t('setup.account.continue') : t('setup.account.create'),
        onClick: () => void handleAccount(),
        loading: createAccount.isPending,
      };
      break;
    case 'pay':
      // Nothing to press: the step moves on by itself once the account is active
      primary = null;
      break;
    case 'kit':
      primary = {
        label: t('setup.account.continue'),
        onClick: () => setStep('device'),
        disabled: !kitConfirmed,
      };
      break;
    default:
      primary = {
        label: t('setup.device.turnOn'),
        onClick: () => void handleFinish(),
        loading: finishSetup.isPending,
        disabled: deviceName.trim() === '',
      };
  }

  const stepNumber = STEPS.indexOf(step) + 1;
  const description =
    step === 'account'
      ? t('setup.account.description')
      : step === 'pay'
        ? t('phone.payWaiting')
        : step === 'kit'
          ? t('setup.kit.description')
          : t('setup.device.description');

  return (
    <>
      <Sheet
        open
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
            {primary ? (
              <Button
                className={`${BUTTON} flex-1`}
                onClick={primary.onClick}
                disabled={primary.disabled}
                loading={primary.loading}
              >
                {primary.label}
              </Button>
            ) : (
              <p
                className="m-0 flex min-h-12 flex-1 items-center justify-center gap-2 text-m-sub text-ink-3"
                role="status"
              >
                <Loader2 aria-hidden className="size-4 animate-spin motion-reduce:animate-none" />
                {t('phone.payWaitingShort')}
              </p>
            )}
          </>
        }
      >
        <StepProgress
          index={stepNumber}
          total={STEPS.length}
          label={t('setup.step', { current: stepNumber, total: STEPS.length })}
        />
        <div className="space-y-3.5">
          {step === 'account' && (
            <>
              {numberBox}
              {accountNumber && (
                <p className="m-0 text-m-sub text-ink-3">{t('setup.account.explain')}</p>
              )}
              <Collapsible>
                <CollapsibleTrigger className="group flex min-h-11 items-center gap-1 text-m-sub font-semibold text-ink-2">
                  <ChevronRight
                    aria-hidden
                    className="size-4 transition-transform group-data-[state=open]:rotate-90"
                  />
                  {t('setup.advanced.toggle')}
                </CollapsibleTrigger>
                <CollapsibleContent className="space-y-1.5 pt-1">
                  <Label htmlFor={`${fieldId}-server`}>{t('setup.advanced.label')}</Label>
                  <Input
                    id={`${fieldId}-server`}
                    value={relayInput}
                    onChange={(event) => {
                      setRelayInput(event.target.value);
                      setRelayError(null);
                    }}
                    placeholder={t('setup.advanced.placeholder')}
                    inputMode="url"
                    aria-invalid={relayError ? true : undefined}
                    className={field48}
                    {...PLAIN_FIELD}
                  />
                  {relayError ? (
                    <p role="alert" className={`m-0 text-loss ${fieldMessage}`}>
                      {relayError}
                    </p>
                  ) : (
                    <p className="m-0 text-m-sub text-ink-4">{t('setup.advanced.hint')}</p>
                  )}
                </CollapsibleContent>
              </Collapsible>
            </>
          )}

          {step === 'pay' && (
            <>
              {pay === 'selfHosted' && (
                <>
                  {numberBox}
                  <p className="m-0 text-m-lead text-ink-2">{t('setup.pay.selfHosted')}</p>
                  <p className="m-0 text-m-sub text-ink-3">{t('setup.pay.selfHostedWaiting')}</p>
                </>
              )}
              {pay === 'unavailable' && (
                <p className="m-0 text-m-lead text-ink-2">{t('phone.payUnavailable')}</p>
              )}
              {pay === 'failed' && (
                <Button
                  variant="secondary"
                  className={`${BUTTON} w-full`}
                  onClick={() => setPay('asking')}
                >
                  {t('pairing.tryAgain')}
                </Button>
              )}
              {status?.devTools && (
                <Button
                  variant="ghost"
                  className={`${BUTTON} w-full`}
                  onClick={() => void handleGrant()}
                  loading={devGrant.isPending}
                >
                  {t('setup.pay.grantTest')}
                </Button>
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
