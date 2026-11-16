/**
 * First-run onboarding.
 *
 * Two columns: the five steps on the left (privacy, account, recovery key,
 * areas, first account), a live miniature of the future overview on the
 * right that follows the chosen areas. The wizard stays mounted after the
 * account is created (steps 4–5 run unlocked); `onFinished` hands control
 * back to the auth page.
 *
 * With sync built in, the first step also offers the join path (spec 2026-11-14 §5.1): the
 * account step asks only this device's password, and Připojení replaces the last two steps. Left
 * without sync, Připojení is followed by the normal steps, with the name asked on Co sledovat.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import { Check, Info, RefreshCw } from 'lucide-react';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input, InputWrap } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  setupSchema,
  MIN_PASSWORD_LENGTH,
  type MenuPreferences,
  type ReplaceOutcomeView,
} from '@shared/schema';
import { FX_SUPPORTED_CODES, type CurrencyCode } from '@shared/currencies';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { MoonMark } from '@/components/common/MoonMark';
import { WizardSteps } from '@/components/common/WizardSteps';
import { rememberGreetingName, rememberGreetingNameFromProfile, useAuth } from '@/hooks/use-auth';
import { replaceOutcomeToast } from '@/components/settings/sync/replace-outcome';
import { useBankAccountMutations } from '@/hooks/use-bank-account-mutations';
import { useDeviceSyncFeature } from '@/hooks/use-device-sync';
import { useFormat } from '@/lib/use-format';
import { authApi, categorizationApi, onboardingApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { cn } from '@/lib/utils';
import { passwordStrength } from '@/utils/password-strength';
import { profileNameSchema } from '@/utils/form-schemas';
import {
  onboardingSteps,
  stepAfterRecovery,
  type OnboardingPath,
  type OnboardingStep,
} from '@/utils/onboarding-steps';
import { todayIsoUtc, utcDayStart } from '@/utils/period';
import {
  RULE_PACK_COUNTRY_IDS,
  countryOptionsFromPacks,
  defaultCurrencyForRegion,
  packIdForRegion,
  regionFromLocale,
} from '@/utils/locale-defaults';
import {
  pickVerificationGroups,
  recoveryKeyGroups,
  verifyGroups,
} from '@/utils/recovery-key-check';
import { StatementImportDialog } from '@/components/bank-accounts/StatementImportDialog';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { RecoveryKeyBox } from '@/components/auth/RecoveryKeyBox';
import { OnboardingPreview, type PreviewAreas } from '@/components/auth/OnboardingPreview';
import { RestoreBackup } from '@/components/common/RestoreBackup';
import { ConnectStep } from '@/components/auth/onboarding/ConnectStep';
import { DevicePasswordForm } from '@/components/auth/onboarding/DevicePasswordForm';
import { StepHead } from '@/components/auth/onboarding/StepHead';
import './onboarding.css';

const NONE = '__none__';

interface OnboardingWizardProps {
  language: string;
  onFinished: () => void;
  /** The live miniature beside (desktop) or below the steps; the phone leaves it out (spec §8.2). */
  preview?: boolean;
  /** Phone: the recovery-key actions are 48 pt buttons (`RecoveryKeyBox touch`); the rest stays as is. */
  touch?: boolean;
}

export function OnboardingWizard({
  language,
  onFinished,
  preview = true,
  touch = false,
}: OnboardingWizardProps) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const { t: tSync } = useTranslation('deviceSync');
  const fmt = useFormat();
  const syncFeature = useDeviceSyncFeature();
  const queryClient = useQueryClient();
  const { appStatus, setupMutation, confirmSetupMutation, recoveryKey, clearRecoveryKey } =
    useAuth();
  const { createAccount } = useBankAccountMutations();
  const wizardRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    wizardRef.current?.querySelector('h1')?.focus({ preventScroll: true });
  }, []);

  const systemRegion = useMemo(() => regionFromLocale(navigator.language), []);
  const [step, setStep] = useState<OnboardingStep>('welcome');
  const [path, setPath] = useState<OnboardingPath>('new');
  /**
   * Join path: the device password typed on the account step, kept in memory only for Připojení
   * (a pairing starts with it, a Kit join sends it) and dropped when that step ends either way.
   */
  const joinPassword = useRef<string | null>(null);
  const [currency, setCurrency] = useState<CurrencyCode>(() =>
    defaultCurrencyForRegion(systemRegion)
  );
  const [country, setCountry] = useState<string>(
    () => packIdForRegion(systemRegion, [...RULE_PACK_COUNTRY_IDS]) ?? NONE
  );
  const privacyPoints = ['encryption', 'localOnly', 'marketData'] as const;

  const countryOptions = useMemo(
    () => countryOptionsFromPacks([...RULE_PACK_COUNTRY_IDS], language),
    [language]
  );
  const countryName = countryOptions.find((o) => o.packId === country)?.name ?? null;

  const stepLabels: Record<OnboardingStep, string> = {
    welcome: t('onboarding.steps.welcome'),
    account: t('onboarding.steps.account'),
    recovery: t('onboarding.steps.recovery'),
    connect: t('onboarding.steps.connect'),
    areas: t('onboarding.steps.areas'),
    firstAccount: t('onboarding.steps.firstAccount'),
  };

  // ---------------- Step 2: account form ----------------
  const setupForm = useForm<z.infer<typeof setupSchema>>({
    resolver: zodResolver(setupSchema),
    defaultValues: { name: '', surname: '', email: '', password: '', confirmPassword: '' },
  });
  // Set once the account exists, when the form is emptied: the name the preview greets by
  const [confirmedName, setConfirmedName] = useState<string | null>(null);
  const passwordValue = setupForm.watch('password');
  const strength = passwordStrength(passwordValue);

  // Join path: only the device password; the synced profile brings name, surname, e-mail and
  // currency (the backend checks only the password and the currency here).
  const submitDevicePassword = (password: string) => {
    joinPassword.current = password;
    setupMutation.mutate({ name: '', surname: '', email: '', password, language, currency });
  };
  const leaveJoinPath = () => {
    joinPassword.current = null;
    // A failed setup keeps the password in the mutation's variables
    setupMutation.reset();
    setPath('new');
    setStep('welcome');
  };

  // Keys are prepared → show the recovery step.
  useEffect(() => {
    if (recoveryKey && step === 'account') setStep('recovery');
  }, [recoveryKey, step]);

  // ---------------- Step 3: recovery key ----------------
  const verifyIndexes = useMemo<[number, number]>(
    () => pickVerificationGroups(recoveryKey ? recoveryKeyGroups(recoveryKey).length : 6),
    [recoveryKey]
  );
  const [typed, setTyped] = useState<[string, string]>(['', '']);
  const verified = recoveryKey ? verifyGroups(recoveryKey, verifyIndexes, typed) : false;
  const showMismatch =
    !verified && typed[0].length >= 4 && typed[1].length >= 4 && recoveryKey !== null;

  const backToAccount = () => {
    joinPassword.current = null;
    clearRecoveryKey();
    // The prepared setup (password and key) lives on in the mutation's variables and data
    setupMutation.reset();
    setTyped(['', '']);
    setStep('account');
    toast(t('onboarding.recovery.discarded'));
  };

  const createAccountNow = () => {
    confirmSetupMutation.mutate(undefined, {
      onSuccess: () => {
        clearRecoveryKey();
        // The password and the key were held in the mutation's variables and data: drop them
        setupMutation.reset();
        // The form held the typed password for the wizard's life: only the first name is kept
        // (the preview greets by it); nothing reads the form's values after this
        setConfirmedName(setupForm.getValues('name'));
        setupForm.reset();
        setStep(stepAfterRecovery(path));
      },
    });
  };

  // ---------------- Join path, step 4: Připojení ----------------
  const unlocked = appStatus === 'unlocked';
  const connectShown = step === 'connect' && unlocked;
  // However the step ends (joined, left without sync, the wizard unmounted), the password goes.
  useEffect(() => {
    if (!connectShown) return;
    return () => {
      joinPassword.current = null;
    };
  }, [connectShown]);

  const declineSync = useCallback(() => {
    joinPassword.current = null;
    setPath('declined');
    setStep('areas');
  }, []);

  // ---------------- Step 4: areas ----------------
  const [areas, setAreas] = useState<PreviewAreas>({
    investments: true,
    realEstate: true,
    liabilities: true,
  });
  const [savingAreas, setSavingAreas] = useState(false);
  // Declined path: the profile has no name yet (the account step asked only the password)
  const nameForm = useForm<z.infer<typeof profileNameSchema>>({
    resolver: zodResolver(profileNameSchema),
    defaultValues: { name: '', surname: '', email: '' },
  });
  const firstName = (
    path === 'declined' ? nameForm.watch('name') : (confirmedName ?? setupForm.watch('name'))
  ).trim();

  const saveAreas = async (names?: z.infer<typeof profileNameSchema>) => {
    if (savingAreas) return;
    setSavingAreas(true);
    try {
      const menuPreferences: MenuPreferences = {
        investments: areas.investments,
        bonds: areas.investments,
        crypto: areas.investments,
        otherAssets: true,
        realEstate: areas.realEstate,
        loans: areas.liabilities,
        insurance: areas.liabilities,
      };
      const profile = await authApi.updateProfile({
        ...(names ? { name: names.name, surname: names.surname, email: names.email ?? '' } : {}),
        menuPreferences,
        ...(country !== NONE ? { country: country.toUpperCase() } : {}),
      });
      queryClient.setQueryData(['user-profile'], profile);
      // The account step asked no name on this path: the lock screen greets by it from now on
      if (names) rememberGreetingName(profile.name);

      if (country !== NONE) {
        const packs = await categorizationApi.getRulePacks();
        const enabled = new Set(packs.filter((p) => p.enabled).map((p) => p.packId));
        enabled.add('global');
        enabled.add(country);
        await categorizationApi.setRulePacksEnabled([...enabled]);
        // The settings card and the rules page read this key (was `rule-packs`).
        queryClient.invalidateQueries({ queryKey: ['rulePacks'] });
      }
      setStep('firstAccount');
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    } finally {
      setSavingAreas(false);
    }
  };

  // ---------------- Step 5: first account ----------------
  const [accountName, setAccountName] = useState('');
  const [accountCurrency, setAccountCurrency] = useState<CurrencyCode>(currency);
  const [accountBalance, setAccountBalance] = useState('');
  const [createdAccountId, setCreatedAccountId] = useState<string | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [finishing, setFinishing] = useState(false);

  useEffect(() => setAccountCurrency(currency), [currency]);

  const finish = async () => {
    setFinishing(true);
    try {
      await onboardingApi.setFlag('onboarding.completedAt', String(Math.floor(Date.now() / 1000)));
    } catch (error) {
      console.error('Failed to store onboarding completion:', error);
    }
    try {
      localStorage.setItem('moony-first-run', '1');
    } catch {
      /* private mode */
    }
    setFinishing(false);
    onFinished();
  };

  const createFirstAccount = async (thenImport: boolean) => {
    if (createdAccountId) {
      if (thenImport) setImportOpen(true);
      else await finish();
      return;
    }

    const name = accountName.trim();
    if (!name) {
      toast.error(tc('validation.nameRequired'));
      return;
    }
    // A typed balance is today's balance: the account's first dated record.
    const typedBalance = accountBalance.trim().replace(',', '.');
    const hasBalance = typedBalance !== '' && Number.isFinite(Number(typedBalance));
    try {
      const account = await createAccount.mutateAsync({
        data: {
          name,
          accountType: 'checking',
          currency: accountCurrency,
          ...(typedBalance !== '' ? { balance: typedBalance } : {}),
          ...(hasBalance ? { balanceAsOf: utcDayStart(todayIsoUtc()) } : {}),
        },
      });
      setCreatedAccountId(account.id);
      if (thenImport) {
        setImportOpen(true);
      } else {
        await finish();
      }
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  const joinedSync = async (outcome: ReplaceOutcomeView) => {
    joinPassword.current = null;
    toast(tSync('join.firstRunDone'));
    // A backup the backend kept (the wizard was left on Připojení for a day) says where it is
    if (outcome.kept) {
      replaceOutcomeToast(tSync, tSync('pairing.joined', { path: outcome.backupPath }), outcome);
    }
    // The profile was created without a name: the synced one is read for the lock screen's greeting
    void rememberGreetingNameFromProfile();
    await finish();
  };

  const balanceNumber = Number(accountBalance.replace(/\s/g, '').replace(',', '.'));
  const previewNetWorth =
    step === 'firstAccount' && accountBalance.trim() !== '' && Number.isFinite(balanceNumber)
      ? fmt.money(balanceNumber, accountCurrency, { decimals: 0 })
      : undefined;

  const nav = (back: (() => void) | null, forward: React.ReactNode) => (
    <div className="mt-2 flex items-center justify-between">
      {back ? (
        <Button type="button" variant="ghost" onClick={back}>
          {tc('buttons.back')}
        </Button>
      ) : (
        <span />
      )}
      {forward}
    </div>
  );

  return (
    <div
      ref={wizardRef}
      className="onboarding-wizard grid min-h-svh grid-cols-1 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]"
    >
      {/* --safe-top / --safe-bottom are 0 on the desktop; on the phone they clear the status bar and the home indicator. */}
      <section className="flex flex-col justify-center px-6 pb-[calc(2rem+var(--safe-bottom))] pt-[calc(5rem+var(--safe-top))] lg:px-10 lg:py-12 xl:px-14">
        <div className="mx-auto w-full max-w-[480px]">
          <div className="mb-9 flex items-center gap-[11px]">
            <MoonMark />
            <div>
              <b className="block text-[18px] font-650 leading-tight tracking-[-0.08em] text-ink">
                moony
              </b>
              <small className="block text-micro font-500 text-ink-4">
                {t('onboarding.brandTagline')}
              </small>
            </div>
          </div>
          <WizardSteps
            className="mb-[26px]"
            steps={onboardingSteps(path).map((id) => ({ id, label: stepLabels[id] }))}
            current={step}
          />

          {/* ---------------- 1. Privacy ---------------- */}
          {step === 'welcome' && (
            <div className="onboarding-step" data-testid="onboarding-welcome">
              <StepHead
                title={t('onboarding.welcome.title')}
                lead={t('onboarding.welcome.subtitle')}
              />
              <div className="mb-[18px] grid gap-2 rounded-r3 bg-well px-4 py-3.5 text-caption text-ink-2">
                {privacyPoints.map((key) => (
                  <div key={key} className="flex items-start gap-2">
                    <Check className="mt-px size-3.5 shrink-0 text-gain" aria-hidden />
                    <span>
                      <b className="font-650 text-ink">
                        {t(`onboarding.welcome.points.${key}.title`)}
                      </b>{' '}
                      {t(
                        key === 'localOnly' && syncFeature
                          ? 'onboarding.welcome.points.localOnly.textSync'
                          : `onboarding.welcome.points.${key}.text`
                      )}
                    </span>
                  </div>
                ))}
              </div>
              <div className="mb-5 grid grid-cols-1 gap-3 sm:grid-cols-2">
                <div className="space-y-1.5">
                  <Label htmlFor="onboarding-currency">{t('onboarding.welcome.currency')}</Label>
                  <CurrencyCombobox
                    id="onboarding-currency"
                    data-testid="onboarding-currency"
                    value={currency}
                    onChange={setCurrency}
                    options={FX_SUPPORTED_CODES}
                  />
                  <p className="text-micro font-500 text-ink-4">
                    {t('onboarding.welcome.currencyHint')}
                  </p>
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="onboarding-country">{t('onboarding.welcome.country')}</Label>
                  <Select value={country} onValueChange={setCountry}>
                    <SelectTrigger id="onboarding-country" data-testid="onboarding-country">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={NONE}>{t('onboarding.welcome.countryNone')}</SelectItem>
                      {countryOptions.map((o) => (
                        <SelectItem key={o.packId} value={o.packId}>
                          {o.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <p className="text-micro font-500 text-ink-4">
                    {t('onboarding.welcome.countryHint')}
                  </p>
                </div>
              </div>
              {nav(
                null,
                <Button data-testid="onboarding-continue" onClick={() => setStep('account')}>
                  {tc('buttons.continue')}
                </Button>
              )}
              {/* Moving from another device or reinstalling: join sync or skip the setup entirely */}
              <RestoreBackup fresh>
                {(pick, busy) => {
                  const restore = (
                    <button
                      type="button"
                      onClick={pick}
                      disabled={busy}
                      className={TEXT_BUTTON}
                      data-testid="onboarding-restore-backup"
                    >
                      {t('onboarding.welcome.restoreBackup')}
                    </button>
                  );
                  return (
                    <p className="m-0 mt-4 border-t border-line-soft pt-4 text-caption font-500 text-ink-3">
                      {syncFeature ? (
                        <>
                          {t('onboarding.welcome.otherDeviceLead')}
                          <span className="mt-1.5 flex flex-wrap gap-x-5 gap-y-1">
                            <button
                              type="button"
                              onClick={() => {
                                setPath('join');
                                setStep('account');
                              }}
                              disabled={busy}
                              className={TEXT_BUTTON}
                              data-testid="onboarding-join-sync"
                            >
                              {t('onboarding.welcome.joinSync')}
                            </button>
                            {restore}
                          </span>
                        </>
                      ) : (
                        <>
                          {t('onboarding.welcome.restoreLead')} {restore}
                        </>
                      )}
                    </p>
                  );
                }}
              </RestoreBackup>
            </div>
          )}

          {/* ---------------- 2. Account (join path: this device's password) ---------------- */}
          {step === 'account' && path === 'join' && (
            <div className="onboarding-step" data-testid="onboarding-join-account">
              <StepHead
                title={t('onboarding.joinAccount.title')}
                lead={t('onboarding.joinAccount.subtitle')}
              />
              <DevicePasswordForm
                pending={setupMutation.isPending}
                onSubmit={submitDevicePassword}
                onBack={leaveJoinPath}
              />
            </div>
          )}

          {/* ---------------- 2. Account ---------------- */}
          {step === 'account' && path !== 'join' && (
            <Form {...setupForm}>
              <form
                onSubmit={setupForm.handleSubmit((data) =>
                  setupMutation.mutate({ ...data, language, currency })
                )}
                className="onboarding-step space-y-4"
                data-testid="onboarding-account"
              >
                <StepHead
                  title={t('onboarding.account.title')}
                  lead={t('onboarding.account.subtitle')}
                />
                <div className="grid grid-cols-2 gap-3">
                  <FormField
                    control={setupForm.control}
                    name="name"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('onboarding.account.name')}</FormLabel>
                        <FormControl>
                          <Input autoFocus autoComplete="given-name" {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={setupForm.control}
                    name="surname"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('onboarding.account.surname')}</FormLabel>
                        <FormControl>
                          <Input autoComplete="family-name" {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                </div>
                <FormField
                  control={setupForm.control}
                  name="email"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel className="flex justify-between">
                        <span>{t('onboarding.account.email')}</span>
                        <span className="font-500 text-ink-5">{tc('labels.optional')}</span>
                      </FormLabel>
                      <FormControl>
                        <Input
                          type="text"
                          placeholder={t('onboarding.account.emailPlaceholder')}
                          {...field}
                        />
                      </FormControl>
                      <FormMessage />
                    </FormItem>
                  )}
                />
                <div className="grid grid-cols-2 gap-3">
                  <FormField
                    control={setupForm.control}
                    name="password"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('onboarding.account.password')}</FormLabel>
                        <FormControl>
                          <PasswordInput autoComplete="new-password" {...field} />
                        </FormControl>
                        {strength && (
                          <FormDescription
                            className={cn(
                              strength === 'weak' && 'text-loss',
                              strength === 'strong' && 'text-gain'
                            )}
                          >
                            {t(`onboarding.account.strength.${strength}`, {
                              min: MIN_PASSWORD_LENGTH,
                            })}
                          </FormDescription>
                        )}
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={setupForm.control}
                    name="confirmPassword"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('onboarding.account.confirmPassword')}</FormLabel>
                        <FormControl>
                          <PasswordInput autoComplete="new-password" {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                </div>
                {nav(
                  () => {
                    // A failed setup keeps the password in the mutation's variables
                    setupMutation.reset();
                    setStep('welcome');
                  },
                  <Button
                    type="submit"
                    disabled={setupMutation.isPending}
                    loading={setupMutation.isPending}
                    data-testid="onboarding-account-continue"
                  >
                    {tc('buttons.continue')}
                  </Button>
                )}
              </form>
            </Form>
          )}

          {/* ---------------- 3. Recovery key ---------------- */}
          {step === 'recovery' && recoveryKey && (
            <div className="onboarding-step" data-testid="onboarding-recovery">
              <StepHead
                title={t('onboarding.recovery.title')}
                lead={t('onboarding.recovery.subtitle')}
              />
              <RecoveryKeyBox recoveryKey={recoveryKey} touch={touch} />
              <div className="mt-[18px] space-y-1.5">
                <Label>
                  {t('onboarding.recovery.verifyLabel', {
                    a: verifyIndexes[0] + 1,
                    b: verifyIndexes[1] + 1,
                  })}
                </Label>
                <div className="grid grid-cols-2 gap-3">
                  {verifyIndexes.map((groupIndex, i) => (
                    <Input
                      key={groupIndex}
                      aria-label={t('onboarding.recovery.groupLabel', { n: groupIndex + 1 })}
                      value={typed[i]}
                      maxLength={6}
                      autoCapitalize="characters"
                      autoComplete="off"
                      className="uppercase tracking-[0.06em] num"
                      onChange={(e) => {
                        const next: [string, string] = [typed[0], typed[1]];
                        next[i] = e.target.value;
                        setTyped(next);
                      }}
                      data-testid={`recovery-verify-${i}`}
                    />
                  ))}
                </div>
                <p
                  className={cn(
                    'text-micro font-500 text-ink-4',
                    showMismatch && 'font-650 text-loss',
                    verified && 'font-650 text-gain'
                  )}
                >
                  {verified
                    ? t('onboarding.recovery.verified')
                    : showMismatch
                      ? t('onboarding.recovery.mismatch')
                      : t('onboarding.recovery.verifyHint')}
                </p>
              </div>
              <p className="mb-2 mt-4 text-micro font-500 text-ink-4">
                {t('recoveryKey.folderNote')}
              </p>
              {nav(
                confirmSetupMutation.isPending ? null : backToAccount,
                confirmSetupMutation.isPending ? (
                  <span className="flex items-center gap-2 text-caption font-500 text-ink-3">
                    <RefreshCw className="size-[13px] animate-spin" aria-hidden />
                    {t('onboarding.recovery.creating')}
                  </span>
                ) : (
                  <Button
                    disabled={!verified}
                    onClick={createAccountNow}
                    data-testid="onboarding-create-account"
                  >
                    {t('onboarding.recovery.create')}
                  </Button>
                )
              )}
            </div>
          )}

          {/* ---------------- 4. Připojení (join path) ---------------- */}
          {connectShown && (
            <ConnectStep
              password={joinPassword.current ?? ''}
              onJoined={(outcome) => void joinedSync(outcome)}
              onDecline={declineSync}
            />
          )}

          {/* ---------------- 4. Areas ---------------- */}
          {step === 'areas' && unlocked && (
            <div className="onboarding-step" data-testid="onboarding-areas">
              <StepHead title={t('onboarding.areas.title')} lead={t('onboarding.areas.subtitle')} />
              {path === 'declined' && (
                <Form {...nameForm}>
                  <form
                    className="mb-5 space-y-4"
                    data-testid="onboarding-areas-name"
                    onSubmit={nameForm.handleSubmit((names) => saveAreas(names))}
                  >
                    <p className="m-0 text-caption font-500 text-ink-3">
                      {t('onboarding.areas.nameLead')}
                    </p>
                    <div className="grid grid-cols-2 gap-3">
                      <FormField
                        control={nameForm.control}
                        name="name"
                        render={({ field }) => (
                          <FormItem>
                            <FormLabel>{t('onboarding.account.name')}</FormLabel>
                            <FormControl>
                              <Input autoFocus autoComplete="given-name" {...field} />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={nameForm.control}
                        name="surname"
                        render={({ field }) => (
                          <FormItem>
                            <FormLabel>{t('onboarding.account.surname')}</FormLabel>
                            <FormControl>
                              <Input autoComplete="family-name" {...field} />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                    </div>
                    <FormField
                      control={nameForm.control}
                      name="email"
                      render={({ field }) => (
                        <FormItem>
                          <FormLabel className="flex justify-between">
                            <span>{t('onboarding.account.email')}</span>
                            <span className="font-500 text-ink-5">{tc('labels.optional')}</span>
                          </FormLabel>
                          <FormControl>
                            <Input
                              type="text"
                              placeholder={t('onboarding.account.emailPlaceholder')}
                              {...field}
                            />
                          </FormControl>
                          <FormMessage />
                        </FormItem>
                      )}
                    />
                  </form>
                </Form>
              )}
              <div className="mb-5 grid grid-cols-1 gap-2.5 sm:grid-cols-2">
                <AreaCard
                  title={t('onboarding.areas.accounts.title')}
                  description={t('onboarding.areas.accounts.description')}
                  checked
                  locked
                />
                <AreaCard
                  title={t('onboarding.areas.investments.title')}
                  description={t('onboarding.areas.investments.description')}
                  checked={areas.investments}
                  onChange={(v) => setAreas((a) => ({ ...a, investments: v }))}
                />
                <AreaCard
                  title={t('onboarding.areas.realEstate.title')}
                  description={t('onboarding.areas.realEstate.description')}
                  checked={areas.realEstate}
                  onChange={(v) => setAreas((a) => ({ ...a, realEstate: v }))}
                />
                <AreaCard
                  title={t('onboarding.areas.liabilities.title')}
                  description={t('onboarding.areas.liabilities.description')}
                  checked={areas.liabilities}
                  onChange={(v) => setAreas((a) => ({ ...a, liabilities: v }))}
                />
              </div>
              {countryName ? (
                <Alert className="mb-[18px]">
                  <Info aria-hidden />
                  <AlertTitle>
                    {t('onboarding.areas.rulesTitle', { country: countryName })}
                  </AlertTitle>
                  <AlertDescription>{t('onboarding.areas.rulesNote')}</AlertDescription>
                </Alert>
              ) : (
                <p className="mb-[18px] text-micro font-500 text-ink-4">
                  {t('onboarding.areas.hint')}
                </p>
              )}
              {nav(
                null,
                <Button
                  onClick={
                    path === 'declined'
                      ? nameForm.handleSubmit((names) => saveAreas(names))
                      : () => void saveAreas()
                  }
                  disabled={savingAreas}
                  loading={savingAreas}
                  data-testid="onboarding-areas-continue"
                >
                  {tc('buttons.continue')}
                </Button>
              )}
            </div>
          )}

          {/* ---------------- 5. First account ---------------- */}
          {step === 'firstAccount' && unlocked && (
            <div className="onboarding-step" data-testid="onboarding-first-account">
              <StepHead
                title={t('onboarding.firstAccount.title')}
                lead={t('onboarding.firstAccount.subtitle')}
              />
              {createdAccountId && (
                <Alert className="mb-5" data-testid="first-account-saved">
                  <Check aria-hidden />
                  <AlertTitle>{t('onboarding.firstAccount.saved')}</AlertTitle>
                  <AlertDescription>{t('onboarding.firstAccount.savedHint')}</AlertDescription>
                </Alert>
              )}
              <div className="space-y-4">
                <div className="space-y-1.5">
                  <Label htmlFor="first-account-name">{t('onboarding.firstAccount.name')}</Label>
                  <Input
                    id="first-account-name"
                    autoFocus
                    value={accountName}
                    disabled={createdAccountId !== null}
                    onChange={(e) => setAccountName(e.target.value)}
                    placeholder={t('onboarding.firstAccount.namePlaceholder')}
                    data-testid="first-account-name"
                  />
                </div>
                <div className="grid grid-cols-[1fr_138px] gap-3">
                  <div className="space-y-1.5">
                    <Label htmlFor="first-account-balance">
                      {t('onboarding.firstAccount.balance')}
                    </Label>
                    <InputWrap unit={accountCurrency}>
                      <Input
                        id="first-account-balance"
                        type="number"
                        inputMode="decimal"
                        step="0.01"
                        value={accountBalance}
                        disabled={createdAccountId !== null}
                        onChange={(e) => setAccountBalance(e.target.value)}
                        placeholder="0"
                      />
                    </InputWrap>
                    <p className="text-micro font-500 text-ink-4">
                      {t('onboarding.firstAccount.balanceHint')}
                    </p>
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="first-account-currency">
                      {t('onboarding.firstAccount.currency')}
                    </Label>
                    <CurrencyCombobox
                      id="first-account-currency"
                      value={accountCurrency}
                      onChange={setAccountCurrency}
                      showName={false}
                      disabled={createdAccountId !== null}
                    />
                  </div>
                </div>
              </div>
              <div className="mb-3.5 mt-5 grid grid-cols-1 gap-3 sm:grid-cols-2">
                <Button
                  variant="outline"
                  onClick={() => createFirstAccount(true)}
                  disabled={createAccount.isPending || finishing}
                  data-testid="first-account-import"
                >
                  {t(
                    createdAccountId
                      ? 'onboarding.firstAccount.importTransactions'
                      : 'onboarding.firstAccount.createAndImport'
                  )}
                </Button>
                <Button
                  onClick={() => createFirstAccount(false)}
                  disabled={createAccount.isPending || finishing}
                  loading={createAccount.isPending}
                  data-testid="first-account-create"
                >
                  {t(
                    createdAccountId
                      ? 'onboarding.firstAccount.continueToApp'
                      : 'onboarding.firstAccount.create'
                  )}
                </Button>
              </div>
              {!createdAccountId && (
                <div className="flex justify-end">
                  <button
                    type="button"
                    onClick={finish}
                    disabled={createAccount.isPending || finishing}
                    className="text-table font-600 text-ink-3 underline-offset-[3px] hover:text-ink hover:underline disabled:pointer-events-none disabled:opacity-50"
                    data-testid="first-account-skip"
                  >
                    {t('onboarding.firstAccount.skip')} →
                  </button>
                </div>
              )}

              {importOpen && createdAccountId && (
                <StatementImportDialog
                  open
                  onOpenChange={(open) => {
                    setImportOpen(open);
                  }}
                  accountId={createdAccountId}
                />
              )}
            </div>
          )}
        </div>
      </section>

      {preview && (
        <OnboardingPreview
          areas={areas}
          name={firstName || undefined}
          netWorth={previewNetWorth}
          caption={
            step === 'firstAccount'
              ? t('onboarding.preview.captionAccount')
              : t('onboarding.preview.caption')
          }
        />
      )}
    </div>
  );
}

const TEXT_BUTTON =
  'font-650 text-ink underline underline-offset-[3px] hover:text-ink-2 disabled:pointer-events-none disabled:opacity-50';

function AreaCard({
  title,
  description,
  checked,
  onChange,
  locked = false,
}: {
  title: string;
  description: string;
  checked: boolean;
  onChange?: (checked: boolean) => void;
  locked?: boolean;
}) {
  const { t } = useTranslation('auth');
  return (
    <label
      className={cn(
        'flex cursor-pointer items-start gap-3 rounded-r3 border border-line-strong bg-paper px-3.5 py-[13px] transition-[border-color,box-shadow] duration-fast hover:border-ink-5',
        checked && 'border-dark shadow-[inset_0_0_0_1px_var(--dark)] hover:border-dark',
        locked && 'cursor-default bg-well'
      )}
    >
      <Checkbox
        checked={checked}
        disabled={locked}
        onCheckedChange={(v) => onChange?.(v === true)}
        className="mt-px"
      />
      <span>
        <b className="block text-body font-650 text-ink">{title}</b>
        <small className="mt-[3px] block text-micro leading-[1.4] text-ink-4">
          {description}
          {locked && ` ${t('onboarding.areas.alwaysOn')}`}
        </small>
      </span>
    </label>
  );
}
