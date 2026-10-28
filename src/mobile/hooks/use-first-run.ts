import { useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { z } from 'zod';
import type { CurrencyCode } from '@shared/currencies';
import type { MenuPreferences, setupSchema } from '@shared/schema';
import { useAuth } from '@/hooks/use-auth';
import { useBankAccountMutations } from '@/hooks/use-bank-account-mutations';
import { authApi, biometricApi, categorizationApi, onboardingApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { todayIsoUtc, utcDayStart } from '@/utils/period';
import {
  RULE_PACK_COUNTRY_IDS,
  countryOptionsFromPacks,
  defaultCurrencyForRegion,
  packIdForRegion,
  regionFromLocale,
} from '@/utils/locale-defaults';
import { BIOMETRIC_STATUS_KEY } from '@/lib/biometrics';
import { firstRunSteps, nextStep, previousStep, type FirstRunStep } from '@/mobile/lib/first-run';

/** What the account form holds: the values that become the profile, and the password twice. */
export type SetupFormValues = z.infer<typeof setupSchema>;

/** The areas the person can leave out; "Účty a cashflow" is always on. */
export interface FirstRunAreas {
  investments: boolean;
  realEstate: boolean;
  liabilities: boolean;
}

/** What the first account is made of: a name, a typed balance (decimal text, or '' for none) and a currency. */
export interface FirstAccountInput {
  name: string;
  balance: string;
  currency: CurrencyCode;
}

/** The steps whose "Pokračovat" or "Teď ne" does nothing but move on. */
const STEPS_THAT_JUST_CONTINUE: readonly FirstRunStep[] = ['privacy', 'biometrics'];

/**
 * The phone's first run, step by step (spec §8.2). The flow is the desktop `OnboardingWizard`'s,
 * copied rather than shared (plan ruling 10): the choices of the first step (main currency and
 * country), the account form, the two-phase setup (`setupMutation` prepares the keys and hands over
 * the recovery key, `confirmSetupMutation` creates the encrypted database), the areas, the first
 * account, and then the two steps the phone adds, Biometrie and Záloha. `onFinished` is called once
 * the last step is done.
 *
 * The state is the flow's: the steps get what they show and the actions to call. `step` moves to
 * `recovery` by itself when the recovery key arrives and to `areas` when the database exists; from
 * then on there is no way back. The Biometrie step is in the list only where the biometry status
 * (it needs no session, so it is read from the first screen) says biometrics can be used.
 */
export function useFirstRun({
  language,
  onFinished,
}: {
  language: string;
  onFinished: () => void;
}) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { setupMutation, confirmSetupMutation, recoveryKey, clearRecoveryKey } = useAuth();
  const { createAccount } = useBankAccountMutations();

  const systemRegion = useMemo(() => regionFromLocale(navigator.language), []);
  const [step, setStep] = useState<FirstRunStep>('privacy');
  const [currency, setCurrency] = useState<CurrencyCode>(() =>
    defaultCurrencyForRegion(systemRegion)
  );
  // A rule-pack id ("cz"), or null for "Jiná / není v seznamu"
  const [country, setCountry] = useState<string | null>(() =>
    packIdForRegion(systemRegion, [...RULE_PACK_COUNTRY_IDS])
  );
  const [areas, setAreas] = useState<FirstRunAreas>({
    investments: true,
    realEstate: true,
    liabilities: true,
  });
  // The form's values survive a trip back from the recovery key (the desktop wizard keeps its form)
  const [accountDraft, setAccountDraft] = useState<SetupFormValues | null>(null);
  const [savingAreas, setSavingAreas] = useState(false);
  const [finishing, setFinishing] = useState(false);

  const countryName = useMemo(
    () =>
      country === null
        ? null
        : (countryOptionsFromPacks([...RULE_PACK_COUNTRY_IDS], language).find(
            (option) => option.packId === country
          )?.name ?? null),
    [country, language]
  );

  // Whether biometrics can be used here. The status needs neither a session nor a profile, so it is
  // known from the first screen and the step count never changes under the progress line. A status
  // that cannot be read (the desktop shell, a browser tab) is no biometry: the six-step list.
  const biometry = useQuery({ queryKey: BIOMETRIC_STATUS_KEY, queryFn: biometricApi.status });
  const biometryAvailable = biometry.data?.available ?? false;
  const steps = useMemo(() => firstRunSteps({ biometryAvailable }), [biometryAvailable]);

  // The keys are prepared: the recovery key is shown
  if (recoveryKey && step === 'account') setStep('recovery');

  const moveOn = (from: FirstRunStep) => {
    const next = nextStep(steps, from);
    if (next) setStep(next);
  };

  // ---------------- Soukromí → Účet ----------------

  const goNext = () => {
    if (STEPS_THAT_JUST_CONTINUE.includes(step)) moveOn(step);
  };

  // ---------------- Účet: prepare the keys ----------------

  const submitAccount = (data: SetupFormValues) => {
    if (setupMutation.isPending) return;
    setAccountDraft(data);
    setupMutation.mutate({ ...data, language, currency });
  };

  // ---------------- Klíč: discard it, or create the database ----------------

  /** Throws the prepared key away; the next "Pokračovat" prepares a new one. */
  const backToAccount = () => {
    clearRecoveryKey();
    setStep('account');
    toast(t('onboarding.recovery.discarded'));
  };

  const createDatabase = () => {
    confirmSetupMutation.mutate(undefined, {
      onSuccess: () => {
        clearRecoveryKey();
        // The password has done its work; nothing needs it on this screen any more
        setAccountDraft(null);
        setStep('areas');
      },
    });
  };

  const goBack = () => {
    if (setupMutation.isPending || confirmSetupMutation.isPending) return;
    const previous = previousStep(steps, step);
    if (previous === null) return;
    if (step === 'recovery') backToAccount();
    else setStep(previous);
  };

  // ---------------- Co sledovat ----------------

  const saveAreas = async () => {
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
        menuPreferences,
        ...(country !== null ? { country: country.toUpperCase() } : {}),
      });
      queryClient.setQueryData(['user-profile'], profile);

      if (country !== null) {
        const packs = await categorizationApi.getRulePacks();
        const enabled = new Set(packs.filter((p) => p.enabled).map((p) => p.packId));
        enabled.add('global');
        enabled.add(country);
        await categorizationApi.setRulePacksEnabled([...enabled]);
        // The settings card and the rules page read this key
        void queryClient.invalidateQueries({ queryKey: ['rulePacks'] });
      }
      moveOn('areas');
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    } finally {
      setSavingAreas(false);
    }
  };

  // ---------------- První účet ----------------

  /** Creates the account (a typed balance is today's balance: its first dated record) and moves on. */
  const createFirstAccount = async ({
    name,
    balance,
    currency: accountCurrency,
  }: FirstAccountInput) => {
    if (createAccount.isPending) return;
    const trimmed = name.trim();
    if (!trimmed) {
      toast.error(tc('validation.nameRequired'));
      return;
    }
    try {
      await createAccount.mutateAsync({
        data: {
          name: trimmed,
          accountType: 'checking',
          currency: accountCurrency,
          ...(balance !== '' ? { balance, balanceAsOf: utcDayStart(todayIsoUtc()) } : {}),
        },
      });
    } catch {
      // The mutation has toasted what went wrong; the step stays to try again
      return;
    }
    moveOn('firstAccount');
  };

  const skipFirstAccount = () => moveOn('firstAccount');

  // ---------------- The end ----------------

  /** Marks the onboarding done and hands control back to the lock page. */
  const finish = async () => {
    if (finishing) return;
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

  return {
    step,
    steps,
    goNext,
    goBack,
    currency,
    setCurrency,
    country,
    setCountry,
    countryName,
    areas,
    setAreas,
    accountDraft,
    setAccountDraft,
    submitAccount,
    backToAccount,
    createDatabase,
    saveAreas,
    createFirstAccount,
    skipFirstAccount,
    finish,
    pending: {
      setup: setupMutation.isPending,
      confirm: confirmSetupMutation.isPending,
      areas: savingAreas,
      account: createAccount.isPending,
      finishing,
    },
    biometryAvailable,
  };
}

export type FirstRunFlow = ReturnType<typeof useFirstRun>;
