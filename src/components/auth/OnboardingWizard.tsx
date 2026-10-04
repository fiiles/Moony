/**
 * First-run onboarding.
 *
 * Two columns: the five steps on the left (privacy, account, recovery key,
 * areas, first account), a live miniature of the future overview on the
 * right that follows the chosen areas. The wizard stays mounted after the
 * account is created (steps 4–5 run unlocked); `onFinished` hands control
 * back to the auth page.
 */
import { useEffect, useMemo, useState } from 'react';
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
import { setupSchema, MIN_PASSWORD_LENGTH, type MenuPreferences } from '@shared/schema';
import { type CurrencyCode } from '@shared/currencies';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { MoonMark } from '@/components/common/MoonMark';
import { WizardSteps } from '@/components/common/WizardSteps';
import { useAuth } from '@/hooks/use-auth';
import { useBankAccountMutations } from '@/hooks/use-bank-account-mutations';
import { useFormat } from '@/lib/use-format';
import { authApi, categorizationApi, onboardingApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { cn } from '@/lib/utils';
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
import { CsvImportDialog } from '@/components/bank-accounts/CsvImportDialog';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { RecoveryKeyBox } from '@/components/auth/RecoveryKeyBox';
import { OnboardingPreview, type PreviewAreas } from '@/components/auth/OnboardingPreview';

type Step = 'welcome' | 'account' | 'recovery' | 'areas' | 'firstAccount';
const STEPS: Step[] = ['welcome', 'account', 'recovery', 'areas', 'firstAccount'];
const NONE = '__none__';

interface OnboardingWizardProps {
  language: string;
  onFinished: () => void;
}

/** Simple length/variety based strength hint — guidance, not a gate. */
function passwordStrength(password: string): 'weak' | 'fair' | 'strong' | null {
  if (!password) return null;
  const variety = [/[a-z]/, /[A-Z]/, /\d/, /[^\w\s]/].filter((re) => re.test(password)).length;
  if (password.length < MIN_PASSWORD_LENGTH) return 'weak';
  if (password.length >= 14 && variety >= 3) return 'strong';
  if (password.length >= 10 || variety >= 3) return 'fair';
  return 'weak';
}

function StepHead({ title, lead }: { title: string; lead: string }) {
  return (
    <>
      <h1 className="m-0 text-[26px] font-650 tracking-[-0.05em] text-ink">{title}</h1>
      <p className="mb-6 mt-2 text-body leading-[1.5] text-ink-3">{lead}</p>
    </>
  );
}

export function OnboardingWizard({ language, onFinished }: OnboardingWizardProps) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const { appStatus, setupMutation, confirmSetupMutation, recoveryKey, clearRecoveryKey } =
    useAuth();
  const { createAccount } = useBankAccountMutations();

  const systemRegion = useMemo(() => regionFromLocale(navigator.language), []);
  const [step, setStep] = useState<Step>('welcome');
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

  const stepLabels: Record<Step, string> = {
    welcome: t('onboarding.steps.welcome'),
    account: t('onboarding.steps.account'),
    recovery: t('onboarding.steps.recovery'),
    areas: t('onboarding.steps.areas'),
    firstAccount: t('onboarding.steps.firstAccount'),
  };

  // ---------------- Step 2: account form ----------------
  const setupForm = useForm<z.infer<typeof setupSchema>>({
    resolver: zodResolver(setupSchema),
    defaultValues: { name: '', surname: '', email: '', password: '', confirmPassword: '' },
  });
  const passwordValue = setupForm.watch('password');
  const firstName = setupForm.watch('name').trim();
  const strength = passwordStrength(passwordValue);

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
    clearRecoveryKey();
    setTyped(['', '']);
    setStep('account');
    toast(t('onboarding.recovery.discarded'));
  };

  const createAccountNow = () => {
    confirmSetupMutation.mutate(undefined, {
      onSuccess: () => {
        clearRecoveryKey();
        setStep('areas');
      },
    });
  };

  // ---------------- Step 4: areas ----------------
  const [areas, setAreas] = useState<PreviewAreas>({
    investments: true,
    realEstate: true,
    liabilities: true,
  });
  const [savingAreas, setSavingAreas] = useState(false);

  const saveAreas = async () => {
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
      const profile = await authApi.updateProfile({ menuPreferences });
      queryClient.setQueryData(['user-profile'], profile);

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
  const [importAccountId, setImportAccountId] = useState<string | null>(null);
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
    const name = accountName.trim();
    if (!name) {
      toast.error(tc('validation.nameRequired'));
      return;
    }
    try {
      const account = await createAccount.mutateAsync({
        data: {
          name,
          accountType: 'checking',
          currency: accountCurrency,
          balance: accountBalance.trim() === '' ? '0' : accountBalance.replace(',', '.'),
        },
      });
      if (thenImport) {
        setImportAccountId(account.id);
      } else {
        await finish();
      }
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  const unlocked = appStatus === 'unlocked';
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
    <div className="grid min-h-screen grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
      <section className="flex flex-col justify-center px-14 py-12">
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
            steps={STEPS.map((id) => ({ id, label: stepLabels[id] }))}
            current={step}
          />

          {/* ---------------- 1. Privacy ---------------- */}
          {step === 'welcome' && (
            <div data-testid="onboarding-welcome">
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
                      {t(`onboarding.welcome.points.${key}.text`)}
                    </span>
                  </div>
                ))}
              </div>
              <div className="mb-5 grid grid-cols-2 gap-3">
                <div className="space-y-1.5">
                  <Label>{t('onboarding.welcome.currency')}</Label>
                  <CurrencyCombobox
                    data-testid="onboarding-currency"
                    value={currency}
                    onChange={setCurrency}
                  />
                  <p className="text-micro font-500 text-ink-4">
                    {t('onboarding.welcome.currencyHint')}
                  </p>
                </div>
                <div className="space-y-1.5">
                  <Label>{t('onboarding.welcome.country')}</Label>
                  <Select value={country} onValueChange={setCountry}>
                    <SelectTrigger data-testid="onboarding-country">
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
            </div>
          )}

          {/* ---------------- 2. Account ---------------- */}
          {step === 'account' && (
            <Form {...setupForm}>
              <form
                onSubmit={setupForm.handleSubmit((data) =>
                  setupMutation.mutate({ ...data, language, currency })
                )}
                className="space-y-4"
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
                  () => setStep('welcome'),
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
            <div data-testid="onboarding-recovery">
              <StepHead
                title={t('onboarding.recovery.title')}
                lead={t('onboarding.recovery.subtitle')}
              />
              <RecoveryKeyBox recoveryKey={recoveryKey} />
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

          {/* ---------------- 4. Areas ---------------- */}
          {step === 'areas' && unlocked && (
            <div data-testid="onboarding-areas">
              <StepHead title={t('onboarding.areas.title')} lead={t('onboarding.areas.subtitle')} />
              <div className="mb-5 grid grid-cols-2 gap-2.5">
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
                  onClick={saveAreas}
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
            <div data-testid="onboarding-first-account">
              <StepHead
                title={t('onboarding.firstAccount.title')}
                lead={t('onboarding.firstAccount.subtitle')}
              />
              <div className="space-y-4">
                <div className="space-y-1.5">
                  <Label htmlFor="first-account-name">{t('onboarding.firstAccount.name')}</Label>
                  <Input
                    id="first-account-name"
                    autoFocus
                    value={accountName}
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
                        onChange={(e) => setAccountBalance(e.target.value)}
                        placeholder="0"
                      />
                    </InputWrap>
                    <p className="text-micro font-500 text-ink-4">
                      {t('onboarding.firstAccount.balanceHint')}
                    </p>
                  </div>
                  <div className="space-y-1.5">
                    <Label>{t('onboarding.firstAccount.currency')}</Label>
                    <CurrencyCombobox value={accountCurrency} onChange={setAccountCurrency} />
                  </div>
                </div>
              </div>
              <div className="mb-3.5 mt-5 grid grid-cols-2 gap-3">
                <Button
                  variant="outline"
                  onClick={() => createFirstAccount(true)}
                  disabled={createAccount.isPending || finishing}
                  data-testid="first-account-import"
                >
                  {t('onboarding.firstAccount.createAndImport')}
                </Button>
                <Button
                  onClick={() => createFirstAccount(false)}
                  disabled={createAccount.isPending || finishing}
                  loading={createAccount.isPending}
                  data-testid="first-account-create"
                >
                  {t('onboarding.firstAccount.create')}
                </Button>
              </div>
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

              {importAccountId && (
                <CsvImportDialog
                  open
                  onOpenChange={(open) => {
                    if (!open) {
                      setImportAccountId(null);
                      void finish();
                    }
                  }}
                  accountId={importAccountId}
                />
              )}
            </div>
          )}
        </div>
      </section>

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
    </div>
  );
}

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
