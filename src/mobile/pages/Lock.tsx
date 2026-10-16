/**
 * Lock screen and first run on the phone (spec §8.1, §8.2; prototype frame 01).
 *
 * The same states as the desktop `auth-page.tsx` — first-run wizard, unlock, three-step recovery —
 * on phone surfaces: the lock is centred under the status bar and above the home indicator, the
 * recovery is a pushed full-height screen with a back button, the wizard drops its live preview.
 * Credential failures show inline under the field; the provider toasts everything else.
 *
 * With biometric unlock switched on, "Odemknout obličejem" comes first and the system prompt opens
 * by itself once; "Použít heslo" shows the password form. A key the keychain can no longer read
 * falls back to the password with the reason under the button, never a loop.
 */
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type MutableRefObject,
  type ReactNode,
} from 'react';
import { useLocation } from 'wouter';
import { zodResolver } from '@hookform/resolvers/zod';
import { useQuery } from '@tanstack/react-query';
import { useForm } from 'react-hook-form';
import { useTranslation } from 'react-i18next';
import { Lock as LockIcon, RefreshCw } from 'lucide-react';
import { z } from 'zod';
import { recoverSchema, unlockSchema } from '@shared/schema';
import { GREETING_NAME_KEY, LOCKED_AT_KEY, useAuth } from '@/hooks/use-auth';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { useFormat } from '@/lib/use-format';
import { isAuthApiError } from '@/lib/api-error';
import { queryClient } from '@/lib/queryClient';
import { biometricApi } from '@/lib/tauri-api';
import { cn } from '@/lib/utils';
import { isSupportedLanguage, type SupportedLanguage } from '@/i18n/index';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { MoonMark } from '@/components/common/MoonMark';
import { LanguageSelect } from '@/components/common/LanguageSelect';
import { OnboardingWelcome } from '@/components/auth/OnboardingWelcome';
import { OnboardingWizard } from '@/components/auth/OnboardingWizard';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { RecoveryKeyBox } from '@/components/auth/RecoveryKeyBox';
import { RecoverySteps } from '@/components/auth/RecoverySteps';
import { BiometryIcon } from '@/mobile/components/BiometryIcon';
import {
  BIOMETRIC_STATUS_KEY,
  biometricCopy,
  biometricFailure,
  canUnlockWithBiometrics,
} from '@/mobile/lib/biometrics';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

type Panel = 'unlock' | 'recover';

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/**
 * Canvas with the glow, under the status bar and above the home indicator. The screen scrolls
 * inside itself (the document never does) and ends where the keyboard begins.
 */
function Frame({ children }: { children: ReactNode }) {
  return (
    <div className="relative flex h-full flex-col overflow-y-auto overscroll-contain bg-canvas bg-[radial-gradient(520px_320px_at_100%_0%,var(--canvas-glow),transparent_70%)] pb-[var(--safe-bottom)] pt-[var(--safe-top)] text-m-body text-ink-2">
      {children}
    </div>
  );
}

/** The phone's primary button (README §13): 48 pt, radius 10, 15 / 600. */
const primaryButton = 'h-12 w-full rounded-[10px] text-m-row';
/**
 * Fields on the lock and recovery screens: 48 pt, 15 pt text. iOS would zoom into a field under
 * 16 px on focus; `maximum-scale=1` in `index.html` is what stops that, at the accessibility cost
 * of no pinch zoom (spec §10.2 scales text with the system text size instead).
 */
const field48 = 'h-12 rounded-[10px] text-m-body';
const fieldMessage = 'text-left text-m-sub';
/** The text links under the form ("Použít heslo", "Zapomenuté heslo?"): 44 pt tall. */
const textLink =
  'mt-2.5 h-11 px-3 text-[13px] font-650 text-ink-2 underline underline-offset-[3px] [-webkit-touch-callout:none] active:text-ink disabled:pointer-events-none disabled:opacity-45';

export default function Lock() {
  const { user, appStatus, recoveryKey } = useAuth();
  const [, setLocation] = useLocation();
  const { i18n } = useTranslation('auth');
  const { updateProfile } = useSettingsMutations();
  const [panel, setPanel] = useState<Panel>('unlock');
  // The biometric prompt opens by itself once per lock screen, not again after the recovery
  // panel or a cancel.
  const autoPrompted = useRef(false);

  // First-run wizard: once `needs_setup` has been seen it stays mounted until it reports
  // completion (its last steps run after the account exists).
  const [sawSetup, setSawSetup] = useState(false);
  const [wizardDone, setWizardDone] = useState(false);
  const [introDone, setIntroDone] = useState(false);
  const finishIntro = useCallback(() => setIntroDone(true), []);
  if (appStatus === 'needs_setup' && !sawSetup) setSawSetup(true);
  const wizardActive = sawSetup && !wizardDone;

  // Language: the one i18n detected (stored choice, else the system language, else English).
  const selectedLanguage: SupportedLanguage = isSupportedLanguage(i18n.language)
    ? i18n.language
    : 'en';
  const handleLanguageChange = (lang: SupportedLanguage) => {
    if (lang === selectedLanguage) return;
    if (appStatus === 'unlocked') {
      // The last onboarding steps already have a profile. Save its language first
      // so I18nProvider does not restore the previous choice when the UI switches.
      updateProfile.mutate({ language: lang }, { onSuccess: () => void i18n.changeLanguage(lang) });
    } else {
      void i18n.changeLanguage(lang);
    }
  };

  useEffect(() => {
    // Only leave once unlocked AND neither the wizard nor a new recovery key is still showing.
    if (user && appStatus === 'unlocked' && !recoveryKey && !wizardActive) {
      setLocation(routes.overview);
    }
  }, [user, appStatus, setLocation, recoveryKey, wizardActive]);

  if (wizardActive) {
    return (
      <div className="relative h-full overflow-y-auto overscroll-contain bg-canvas">
        <header className="pointer-events-none absolute inset-x-0 top-[var(--safe-top)] z-10 flex h-11 items-center justify-end px-3">
          <LanguageSelect
            className="pointer-events-auto h-11"
            showFormatSample={false}
            value={selectedLanguage}
            onChange={handleLanguageChange}
            disabled={updateProfile.isPending}
          />
        </header>
        {introDone ? (
          <OnboardingWizard
            language={selectedLanguage}
            preview={false}
            touch
            onFinished={() => {
              setWizardDone(true);
              setLocation(routes.overview);
            }}
          />
        ) : (
          <OnboardingWelcome onContinue={finishIntro} />
        )}
      </div>
    );
  }

  // A new recovery key (step 3) is always shown on its own screen, wherever the flow was left.
  if (recoveryKey)
    return <NewKeyScreen recoveryKey={recoveryKey} onDone={() => setPanel('unlock')} />;
  if (panel === 'recover') return <RecoverScreen onBack={() => setPanel('unlock')} />;
  return (
    <UnlockScreen
      language={selectedLanguage}
      onLanguageChange={handleLanguageChange}
      onForgot={() => setPanel('recover')}
      autoPrompted={autoPrompted}
    />
  );
}

function UnlockScreen({
  language,
  onLanguageChange,
  onForgot,
  autoPrompted,
}: {
  language: SupportedLanguage;
  onLanguageChange: (language: SupportedLanguage) => void;
  onForgot: () => void;
  autoPrompted: MutableRefObject<boolean>;
}) {
  const { unlockMutation, unlockWithBiometricsMutation } = useAuth();
  const { t } = useTranslation('auth');
  const { t: tm } = useTranslation('mobile');
  const fmt = useFormat();

  const form = useForm<z.infer<typeof unlockSchema>>({
    resolver: zodResolver(unlockSchema),
    defaultValues: { password: '' },
  });

  // The mutation lives in AuthProvider, so `isPending` survives re-renders of this page. While
  // it runs (the KDF takes seconds), the form stays put.
  const pending = unlockMutation.isPending;
  const biometricPending = unlockWithBiometricsMutation.isPending;

  // Biometrics switched on: "Odemknout obličejem" first, the password behind a link. Until the
  // status is known neither path shows, so the password form never flashes first.
  const biometrics = useQuery({ queryKey: BIOMETRIC_STATUS_KEY, queryFn: biometricApi.status });
  const status = biometrics.data;
  const biometricReady = canUnlockWithBiometrics(status);
  const [passwordChosen, setPasswordChosen] = useState(false);
  const [biometricMessage, setBiometricMessage] = useState<string | null>(null);
  const showBiometric = biometricReady && !passwordChosen;
  const copy = biometricCopy(status?.biometryType ?? 'none');

  const { mutate: unlockWithBiometrics } = unlockWithBiometricsMutation;
  const promptBiometrics = useCallback(() => {
    unlockWithBiometrics(tm('lock.reason'), {
      onError: (error) => {
        const failure = biometricFailure(isAuthApiError(error) ? error.key : undefined);
        // A dismissed prompt changes nothing, and nothing prompts again by itself.
        if (failure === 'cancelled') return;
        if (failure !== 'other') setBiometricMessage(error.message);
        setPasswordChosen(true);
        // Invalidated: the backend switched biometrics off; the screen follows.
        if (failure === 'invalidated') {
          void queryClient.invalidateQueries({ queryKey: BIOMETRIC_STATUS_KEY });
        }
      },
    });
  }, [unlockWithBiometrics, tm]);
  /** A tap on "Odemknout obličejem": the last message goes, the prompt opens. */
  const tapBiometrics = () => {
    if (biometricPending) return;
    setBiometricMessage(null);
    setPasswordChosen(false);
    promptBiometrics();
  };

  // The prompt opens by itself once, as soon as biometrics turn out to be on.
  useEffect(() => {
    if (!biometricReady || autoPrompted.current) return;
    autoPrompted.current = true;
    promptBiometrics();
  }, [biometricReady, autoPrompted, promptBiometrics]);

  const submit = (data: z.infer<typeof unlockSchema>) => {
    if (pending || biometricPending) return;
    unlockMutation.mutate(data, {
      onError: (error) => {
        if (!isAuthApiError(error)) return;
        form.resetField('password');
        form.setError('password', { type: 'server', message: error.message });
        form.setFocus('password');
      },
    });
  };

  const greetingName = readStorage(GREETING_NAME_KEY);
  const lockedAt = Number(readStorage(LOCKED_AT_KEY)) || null;
  const lockedToday =
    lockedAt !== null && new Date(lockedAt).toDateString() === new Date().toDateString();

  return (
    <Frame>
      <main className="flex flex-1 flex-col items-center px-7 pb-6 pt-4 text-center">
        {/* `my-auto` centres the group and still lets it scroll from the top when the keyboard is up. */}
        <div className="my-auto flex w-full flex-col items-center">
          <div className="mb-11 flex items-center gap-2.5">
            <MoonMark size={36} />
            <b className="text-m-title text-ink">moony</b>
          </div>

          <div className="mb-[18px] grid size-[76px] place-items-center rounded-[22px] border border-line bg-paper text-ink shadow-e2">
            {showBiometric && status ? (
              <BiometryIcon type={status.biometryType} className="size-9" />
            ) : (
              <LockIcon className="size-9" strokeWidth={1.5} aria-hidden />
            )}
          </div>

          <h1 className="m-0 text-[22px] font-650 tracking-[-0.04em] text-ink">
            {greetingName ? t('login.titleNamed', { name: greetingName }) : t('login.title')}
          </h1>
          <p className="mb-[26px] mt-1.5 text-m-sub text-ink-3">
            {lockedAt !== null
              ? lockedToday
                ? t('lock.lockedToday', { time: fmt.time(lockedAt / 1000) })
                : t('lock.lockedOn', { date: fmt.dateTime(lockedAt / 1000) })
              : t('lock.encrypted')}
          </p>

          {biometrics.isPending ? null : showBiometric && status ? (
            <div className="w-full max-w-[360px] space-y-3">
              <Button
                className={`${primaryButton} [&_svg]:size-[18px]`}
                aria-busy={biometricPending || undefined}
                onClick={tapBiometrics}
              >
                <BiometryIcon type={status.biometryType} />
                {tm(copy.unlock)}
              </Button>
              <BiometricMessage message={biometricMessage} />
              <button
                type="button"
                onClick={() => {
                  setBiometricMessage(null);
                  setPasswordChosen(true);
                }}
                disabled={biometricPending}
                className={textLink}
              >
                {tm('lock.usePassword')}
              </button>
            </div>
          ) : (
            <>
              <Form {...form}>
                <form
                  onSubmit={form.handleSubmit(submit)}
                  aria-busy={pending}
                  className="w-full max-w-[360px] space-y-3"
                >
                  <FormField
                    control={form.control}
                    name="password"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel className="sr-only">{t('login.password')}</FormLabel>
                        <FormControl>
                          <PasswordInput
                            touch
                            autoComplete="current-password"
                            enterKeyHint="go"
                            placeholder={t('login.password')}
                            readOnly={pending}
                            className={field48}
                            {...field}
                          />
                        </FormControl>
                        {!pending && <FormMessage className={fieldMessage} />}
                      </FormItem>
                    )}
                  />
                  <Button
                    type="submit"
                    className={primaryButton}
                    disabled={pending || biometricPending}
                    loading={pending}
                  >
                    {pending ? t('login.unlocking') : t('login.unlock')}
                  </Button>
                  <div aria-live="polite" className="empty:hidden">
                    {pending && (
                      <p className="m-0 flex items-start justify-center gap-2 text-m-sub text-ink-3">
                        <RefreshCw className="mt-px size-3.5 shrink-0 animate-spin" aria-hidden />
                        <span>{t('login.decrypting')}</span>
                      </p>
                    )}
                  </div>
                  {!pending && <BiometricMessage message={biometricMessage} />}
                </form>
              </Form>

              {biometricReady && (
                <button
                  type="button"
                  onClick={tapBiometrics}
                  disabled={pending || biometricPending}
                  className={textLink}
                >
                  {tm(copy.unlock)}
                </button>
              )}
              <button
                type="button"
                onClick={onForgot}
                disabled={pending || biometricPending}
                className={cn(textLink, biometricReady && 'mt-0')}
              >
                {t('login.forgotPassword')}
              </button>
            </>
          )}
        </div>
      </main>

      <footer className="flex flex-col items-center gap-2 px-7 pb-6">
        <LanguageSelect className="h-11" value={language} onChange={onLanguageChange} />
        <span className="inline-flex items-center gap-1 text-m-caption text-ink-4">
          <LockIcon className="size-[11px]" aria-hidden />
          {tm('lock.caption')}
        </span>
      </footer>
    </Frame>
  );
}

/**
 * Why the biometric path is not there right now ("Biometrie se změnila, odemkněte heslem a
 * zapněte ji znovu."): under the primary, announced politely.
 */
function BiometricMessage({ message }: { message: string | null }) {
  return (
    <p role="status" className="m-0 text-balance text-m-sub text-ink-2 empty:hidden">
      {message}
    </p>
  );
}

/** Recovery steps 1 and 2: the old recovery key and the new password (a pushed screen). */
function RecoverScreen({ onBack }: { onBack: () => void }) {
  const { recoverMutation } = useAuth();
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');

  const form = useForm<z.infer<typeof recoverSchema>>({
    resolver: zodResolver(recoverSchema),
    defaultValues: { recoveryKey: '', newPassword: '', confirmPassword: '' },
  });
  const pending = recoverMutation.isPending;

  const submit = (data: z.infer<typeof recoverSchema>) => {
    if (pending) return;
    recoverMutation.mutate(data, {
      onError: (error) => {
        if (!isAuthApiError(error)) return;
        form.setError('recoveryKey', { type: 'server', message: error.message });
        form.setFocus('recoveryKey', { shouldSelect: true });
      },
    });
  };

  return (
    <Screen
      title={t('recover.title')}
      tabBar={false}
      leading={
        <NavTextButton label={tc('buttons.back')} onClick={onBack} disabled={pending} chevron />
      }
    >
      <div className="mx-0.5 mt-2">
        <RecoverySteps current={pending ? 2 : 1} />
        <p className="mb-5 mt-0 text-m-body text-ink-3">{t('recover.lead')}</p>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(submit)} aria-busy={pending} className="space-y-4">
            <FormField
              control={form.control}
              name="recoveryKey"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('recover.recoveryKey')}</FormLabel>
                  <FormControl>
                    <Input
                      placeholder="XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"
                      autoComplete="off"
                      autoCapitalize="characters"
                      autoCorrect="off"
                      spellCheck={false}
                      enterKeyHint="next"
                      className={`${field48} uppercase tracking-[0.06em] num`}
                      readOnly={pending}
                      {...field}
                    />
                  </FormControl>
                  <p className="!mt-1.5 text-m-sub text-ink-4">{t('recover.keyHint')}</p>
                  {!pending && <FormMessage className={fieldMessage} />}
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="newPassword"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('recover.newPassword')}</FormLabel>
                  <FormControl>
                    <PasswordInput
                      touch
                      placeholder={t('recover.newPasswordPlaceholder')}
                      autoComplete="new-password"
                      className={field48}
                      {...field}
                    />
                  </FormControl>
                  <FormMessage className={fieldMessage} />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="confirmPassword"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('recover.confirmPassword')}</FormLabel>
                  <FormControl>
                    <PasswordInput
                      touch
                      autoComplete="new-password"
                      enterKeyHint="go"
                      className={field48}
                      {...field}
                    />
                  </FormControl>
                  <FormMessage className={fieldMessage} />
                </FormItem>
              )}
            />
            <Button type="submit" className={primaryButton} disabled={pending} loading={pending}>
              {pending ? t('recover.recovering') : t('recover.recoverAccount')}
            </Button>
          </form>
        </Form>
        <p className="mb-0 mt-4 flex items-start gap-1.5 text-m-sub text-ink-4">
          {pending && <RefreshCw className="mt-px size-3.5 shrink-0 animate-spin" aria-hidden />}
          <span>{pending ? t('recover.verifying') : t('recover.takesSeconds')}</span>
        </p>
      </div>
    </Screen>
  );
}

/** Recovery step 3: the new recovery key, shown once, then confirmed. */
function NewKeyScreen({ recoveryKey, onDone }: { recoveryKey: string; onDone: () => void }) {
  const { confirmRecoveryMutation, clearRecoveryKey } = useAuth();
  const { t } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const { t: tm } = useTranslation('mobile');
  const [keySaved, setKeySaved] = useState(false);
  const pending = confirmRecoveryMutation.isPending;

  const leave = () => {
    clearRecoveryKey();
    setKeySaved(false);
    onDone();
  };

  return (
    <Screen
      title={t('recover.newKeyTitle')}
      tabBar={false}
      leading={<NavTextButton label={tc('buttons.cancel')} onClick={leave} disabled={pending} />}
    >
      <div className="mx-0.5 mt-2">
        <RecoverySteps current={3} />
        <p className="mb-5 mt-0 text-m-body text-ink-3">{t('recover.newKeyLead')}</p>
        <RecoveryKeyBox recoveryKey={recoveryKey} touch />
        {/* The whole row is the label, 44 pt tall at least, so one line of text is still easy to hit. */}
        <label className="my-3 flex min-h-11 cursor-pointer items-center gap-3 py-1 text-m-body text-ink-2">
          <Checkbox
            className="size-5"
            checked={keySaved}
            onCheckedChange={(v) => setKeySaved(v === true)}
          />
          {tm('lock.keySaved')}
        </label>
        <Button
          className={primaryButton}
          disabled={!keySaved || pending}
          loading={pending}
          onClick={() =>
            confirmRecoveryMutation.mutate(undefined, {
              onSuccess: () => {
                clearRecoveryKey();
                setKeySaved(false);
                onDone();
              },
            })
          }
        >
          {pending ? t('recover.recovering') : t('recover.continueToApp')}
        </Button>
        <p className="mb-0 mt-4 text-m-sub text-ink-4">{t('recoveryKey.shownOnce')}</p>
      </div>
    </Screen>
  );
}
