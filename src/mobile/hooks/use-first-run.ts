import { useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { z } from 'zod';
import type { CurrencyCode } from '@shared/currencies';
import type { setupSchema } from '@shared/schema';
import { rememberGreetingName, rememberGreetingNameFromProfile, useAuth } from '@/hooks/use-auth';
import { useBankAccountMutations } from '@/hooks/use-bank-account-mutations';
import { authApi, biometricApi, categorizationApi, onboardingApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { profileNameSchema } from '@/utils/form-schemas';
import type { OnboardingPath } from '@/utils/onboarding-steps';
import { todayIsoUtc, utcDayStart } from '@/utils/period';
import {
  RULE_PACK_COUNTRY_IDS,
  countryOptionsFromPacks,
  defaultCurrencyForRegion,
  packIdForRegion,
  regionFromLocale,
} from '@/utils/locale-defaults';
import { BIOMETRIC_STATUS_KEY } from '@/lib/biometrics';
import { preferencesWithAreas, type TrackedAreas } from '@/mobile/lib/tracked-areas';
import { exportsOnThisDevice } from '@/mobile/lib/export-file';
import {
  firstRunSteps,
  nextStep,
  previousStep,
  type FirstRunExit,
  type FirstRunStep,
} from '@/mobile/lib/first-run';

/** What the account form holds: the values that become the profile, and the password twice. */
export type SetupFormValues = z.infer<typeof setupSchema>;

/** The areas the person can leave out; "Účty a cashflow" is always on. */
export type FirstRunAreas = TrackedAreas;

/** What the first account is made of: a name, a typed balance (decimal text, or '' for none) and a currency. */
export interface FirstAccountInput {
  name: string;
  balance: string;
  currency: CurrencyCode;
}

/** The name the declined path asks on Co sledovat (the join path's account step asked none). */
export interface ProfileNameValues {
  name: string;
  surname: string;
  email: string;
}

/** The steps whose "Pokračovat" or "Teď ne" does nothing but move on. */
const STEPS_THAT_JUST_CONTINUE: readonly FirstRunStep[] = ['privacy', 'biometrics'];

/**
 * The phone's first run, step by step (spec §8.2). The flow is the desktop `OnboardingWizard`'s,
 * copied rather than shared (plan ruling 10): the choices of the first step (main currency and
 * country), the account form, the two-phase setup (`setupMutation` prepares the keys and hands over
 * the recovery key, `confirmSetupMutation` creates the encrypted database), the areas, the first
 * account, and then the two steps the phone adds, Biometrie and Záloha. `onFinished` is called once
 * the last step is done, with where the run ends (the overview, or Settings → Synchronizace).
 *
 * With sync built in, the first step also offers the join path (spec 2026-11-14 §5.2): the account
 * step asks only this device's password, which is kept in memory for Připojení and dropped when
 * that step ends either way; a join ends the run after Biometrie, Připojení left without sync
 * continues with the normal steps, the name asked on Co sledovat.
 *
 * The state is the flow's: the steps get what they show and the actions to call. `step` moves to
 * `recovery` by itself when the recovery key arrives and to the next step (`connect` on the join
 * path, else `areas`) when the database exists; from then on there is no way back. The Biometrie step is in the list only where the biometry status
 * (it needs no session, so it is read from the first screen) says biometrics can be used.
 */
export function useFirstRun({
  language,
  onFinished,
}: {
  language: string;
  onFinished: (to: FirstRunExit) => void;
}) {
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const { t: tSync } = useTranslation('deviceSync');
  const queryClient = useQueryClient();
  const { appStatus, setupMutation, confirmSetupMutation, recoveryKey, clearRecoveryKey } =
    useAuth();
  const { createAccount } = useBankAccountMutations();

  const systemRegion = useMemo(() => regionFromLocale(navigator.language), []);
  const [step, setStep] = useState<FirstRunStep>('privacy');
  const [path, setPath] = useState<OnboardingPath>('new');
  /**
   * Join path: the device password typed on the account step, kept in memory only for Připojení
   * (a scanned pairing starts with it, a Kit join sends it) and dropped when that step ends.
   */
  const [joinPassword, setJoinPassword] = useState<string | null>(null);
  const [profileName, setProfileName] = useState<ProfileNameValues>({
    name: '',
    surname: '',
    email: '',
  });
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
  // Where a backup cannot be made (not iOS yet) the run ends without the "Záloha" step (audit C7)
  const canBackup = exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '');
  const steps = useMemo(
    () => firstRunSteps({ biometryAvailable, canBackup, path }),
    [biometryAvailable, canBackup, path]
  );

  // The keys are prepared: the recovery key is shown
  if (recoveryKey && step === 'account') setStep('recovery');
  // A lock closes the database under Připojení: the step can join no more, the password goes
  if (step === 'connect' && appStatus === 'locked' && joinPassword !== null) setJoinPassword(null);

  /** The next step, or the end of the run after the last one (`finish`). */
  const moveOn = (from: FirstRunStep) => {
    const next = nextStep(steps, from);
    if (next) setStep(next);
    else void finish();
  };

  // ---------------- Soukromí → Účet ----------------

  const goNext = () => {
    if (STEPS_THAT_JUST_CONTINUE.includes(step)) moveOn(step);
  };

  /** "Připojit k synchronizaci": the join path's account step (this device's password only). */
  const startJoin = () => {
    if (step !== 'privacy') return;
    setPath('join');
    setStep('account');
  };

  // ---------------- Účet: prepare the keys ----------------

  const submitAccount = (data: SetupFormValues) => {
    if (setupMutation.isPending) return;
    setAccountDraft(data);
    setupMutation.mutate({ ...data, language, currency });
  };

  /**
   * Join path: only the device password; the synced profile brings name, surname, e-mail and
   * currency (the backend checks only the password and the currency here). The password stays in
   * memory for Připojení.
   */
  const submitJoinPassword = (password: string) => {
    if (setupMutation.isPending) return;
    setJoinPassword(password);
    setupMutation.mutate({ name: '', surname: '', email: '', password, language, currency });
  };

  // ---------------- Klíč: discard it, or create the database ----------------

  /** Throws the prepared key away; the next "Pokračovat" prepares a new one. */
  const backToAccount = () => {
    setJoinPassword(null);
    clearRecoveryKey();
    // The prepared setup (password and key) lives on in the mutation's variables and data
    setupMutation.reset();
    setStep('account');
    toast(t('onboarding.recovery.discarded'));
  };

  const createDatabase = () => {
    confirmSetupMutation.mutate(undefined, {
      onSuccess: () => {
        clearRecoveryKey();
        // The password has done its work; nothing needs it on this screen any more (the mutation
        // held it, and the key, in its variables and data). Only the join path keeps it, in
        // `joinPassword`, for Připojení.
        setAccountDraft(null);
        setupMutation.reset();
        setStep(nextStep(steps, 'recovery') ?? 'areas');
      },
    });
  };

  const goBack = () => {
    if (setupMutation.isPending || confirmSetupMutation.isPending) return;
    const previous = previousStep(steps, step);
    if (previous === null) return;
    if (step === 'recovery') {
      backToAccount();
      return;
    }
    if (step === 'account') {
      // A failed setup keeps the password in the mutation's variables: it goes with the step
      setupMutation.reset();
      // Back from the join path's account step: the first step offers both ways again
      if (path === 'join') {
        setJoinPassword(null);
        setPath('new');
      }
    }
    setStep(previous);
  };

  // ---------------- Připojení (join path) ----------------

  /** The join is done and nothing needs the password any more (a kept backup's sheet comes first). */
  const forgetJoinPassword = () => setJoinPassword(null);

  /** The join is done: the password goes, the run ends after Biometrie (or now). */
  const joined = () => {
    setJoinPassword(null);
    // The profile was created without a name: the synced one is read for the lock screen's greeting
    void rememberGreetingNameFromProfile();
    toast(tSync('join.firstRunDone'));
    moveOn('connect');
  };

  /** "Pokračovat bez synchronizace": the password goes, the normal steps follow. */
  const declineJoin = () => {
    setJoinPassword(null);
    setPath('declined');
    setStep('areas');
  };

  // ---------------- Co sledovat ----------------

  const saveAreas = async () => {
    if (savingAreas) return;
    // Declined path: the profile has no name yet (the account step asked only the password)
    let names: ProfileNameValues | null = null;
    if (path === 'declined') {
      const parsed = profileNameSchema.safeParse(profileName);
      if (!parsed.success) {
        toast.error(tc(parsed.error.issues[0]?.message ?? 'status.error'));
        return;
      }
      names = { ...parsed.data, email: parsed.data.email ?? '' };
    }
    setSavingAreas(true);
    try {
      const menuPreferences = preferencesWithAreas(null, areas);
      const profile = await authApi.updateProfile({
        ...(names ?? {}),
        menuPreferences,
        ...(country !== null ? { country: country.toUpperCase() } : {}),
      });
      queryClient.setQueryData(['user-profile'], profile);
      // The account step asked no name on this path: the lock screen greets by it from now on
      if (names) rememberGreetingName(profile.name);

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

  /** Marks the onboarding done and hands control back to the lock page, which goes to `to`. */
  const finish = async (to: FirstRunExit = 'overview') => {
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
    onFinished(to);
  };

  return {
    step,
    steps,
    path,
    goNext,
    goBack,
    startJoin,
    submitJoinPassword,
    joinPassword,
    joined,
    forgetJoinPassword,
    declineJoin,
    profileName,
    setProfileName,
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
