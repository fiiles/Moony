/**
 * Lock screen (design system prototype lock.html): one card with the brand,
 * a greeting by name, the password with a decrypting state and an inline
 * error, recovery as a three-step flow inside the same card, and the
 * language as a segmented control. The first run renders the onboarding
 * wizard instead.
 */
import { useEffect, useState } from 'react';
import { useLocation } from 'wouter';
import { zodResolver } from '@hookform/resolvers/zod';
import { useForm } from 'react-hook-form';
import { useTranslation } from 'react-i18next';
import { CircleAlert, RefreshCw } from 'lucide-react';
import { z } from 'zod';
import { recoverSchema, unlockSchema } from '@shared/schema';
import { GREETING_NAME_KEY, LOCKED_AT_KEY, useAuth } from '@/hooks/use-auth';
import { useFormat } from '@/lib/use-format';
import { isAuthApiError } from '@/lib/api-error';
import { SUPPORTED_LANGUAGES, type SupportedLanguage } from '@/i18n/index';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { MoonMark } from '@/components/common/MoonMark';
import { OnboardingWizard } from '@/components/auth/OnboardingWizard';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { LanguageSegment } from '@/components/auth/LanguageSegment';
import { RecoveryKeyBox } from '@/components/auth/RecoveryKeyBox';
import { cn } from '@/lib/utils';

type Panel = 'unlock' | 'recover' | 'newKey';

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Three dots: which part of the recovery flow the user is in. */
function RecoverySteps({ current }: { current: 1 | 2 | 3 }) {
  const { t } = useTranslation('auth');
  const labels = [t('recover.steps.key'), t('recover.steps.password'), t('recover.steps.newKey')];
  return (
    <ol className="mb-[18px] flex gap-1.5 text-micro font-600 text-ink-4">
      {labels.map((label, i) => {
        const on = i + 1 <= current;
        return (
          <li key={label} className={cn('flex items-center gap-1.5', on && 'text-ink')}>
            <i className={cn('block size-1.5 rounded-full bg-well-3', on && 'bg-dark')} />
            {label}
          </li>
        );
      })}
    </ol>
  );
}

export default function AuthPage() {
  const {
    user,
    appStatus,
    unlockMutation,
    recoverMutation,
    confirmRecoveryMutation,
    recoveryKey,
    clearRecoveryKey,
  } = useAuth();
  const [, setLocation] = useLocation();
  const { t, i18n } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const [panel, setPanel] = useState<Panel>('unlock');
  const [keySaved, setKeySaved] = useState(false);
  const [version, setVersion] = useState<string | null>(null);

  // First-run wizard: once `needs_setup` has been seen it stays mounted until
  // it reports completion (its last steps run after the account exists).
  const [sawSetup, setSawSetup] = useState(false);
  const [wizardDone, setWizardDone] = useState(false);
  if (appStatus === 'needs_setup' && !sawSetup) setSawSetup(true);
  const wizardActive = sawSetup && !wizardDone;

  // Language: the stored choice, else English
  const [selectedLanguage, setSelectedLanguage] = useState<SupportedLanguage>(() => {
    const stored = readStorage('moony-language');
    return stored && SUPPORTED_LANGUAGES.includes(stored as SupportedLanguage)
      ? (stored as SupportedLanguage)
      : 'en';
  });
  const handleLanguageChange = (lang: SupportedLanguage) => {
    setSelectedLanguage(lang);
    i18n.changeLanguage(lang);
    try {
      localStorage.setItem('moony-language', lang);
    } catch {
      /* private mode */
    }
  };

  useEffect(() => {
    import('@tauri-apps/api/app')
      .then(({ getVersion }) => getVersion())
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  useEffect(() => {
    // Only redirect if unlocked AND the first-run wizard is not still running
    if (user && appStatus === 'unlocked' && !recoveryKey && !wizardActive) {
      setLocation('/');
    }
  }, [user, appStatus, setLocation, recoveryKey, wizardActive]);

  const unlockForm = useForm<z.infer<typeof unlockSchema>>({
    resolver: zodResolver(unlockSchema),
    defaultValues: { password: '' },
  });
  const recoverForm = useForm<z.infer<typeof recoverSchema>>({
    resolver: zodResolver(recoverSchema),
    defaultValues: { recoveryKey: '', newPassword: '', confirmPassword: '' },
  });

  // The unlock/recover mutations live in AuthProvider, so `isPending` survives re-renders of this
  // page. While either runs (the KDF takes seconds), the panels stay put.
  const unlockPending = unlockMutation.isPending;
  const recoverPending = recoverMutation.isPending;

  // Credential failures are shown inline under the field; the provider only toasts the rest.
  const submitUnlock = (data: z.infer<typeof unlockSchema>) => {
    if (unlockPending) return;
    unlockMutation.mutate(data, {
      onError: (error) => {
        if (!isAuthApiError(error)) return;
        unlockForm.resetField('password');
        unlockForm.setError('password', { type: 'server', message: error.message });
        unlockForm.setFocus('password');
      },
    });
  };
  const submitRecover = (data: z.infer<typeof recoverSchema>) => {
    if (recoverPending) return;
    recoverMutation.mutate(data, {
      onError: (error) => {
        if (!isAuthApiError(error)) return;
        recoverForm.setError('recoveryKey', { type: 'server', message: error.message });
        recoverForm.setFocus('recoveryKey', { shouldSelect: true });
      },
    });
  };

  const showPanel: Panel = recoveryKey ? 'newKey' : panel;
  const greetingName = readStorage(GREETING_NAME_KEY);
  const lockedAt = Number(readStorage(LOCKED_AT_KEY)) || null;
  const lockedToday =
    lockedAt !== null && new Date(lockedAt).toDateString() === new Date().toDateString();

  if (wizardActive) {
    return (
      <div className="relative min-h-screen bg-canvas">
        <LanguageSegment
          className="absolute right-6 top-[22px] z-10"
          language={selectedLanguage}
          onLanguageChange={handleLanguageChange}
        />
        <OnboardingWizard
          language={selectedLanguage}
          onFinished={() => {
            setWizardDone(true);
            setLocation('/');
          }}
        />
      </div>
    );
  }

  return (
    <div className="relative grid min-h-screen place-items-center bg-canvas px-6 py-10">
      <LanguageSegment
        className="absolute right-6 top-[22px] z-10"
        language={selectedLanguage}
        onLanguageChange={handleLanguageChange}
      />

      <Card className="w-[420px] max-w-full px-9 pb-[30px] pt-[34px]">
        <div className="mb-[26px] flex items-center gap-3">
          <MoonMark size={36} />
          <div>
            <b className="block text-[20px] font-650 leading-tight tracking-[-0.08em] text-ink">
              moony
            </b>
            <small className="block text-micro font-500 text-ink-4">{t('lock.brandTagline')}</small>
          </div>
        </div>

        {showPanel === 'unlock' && (
          <>
            <h1 className="m-0 text-[24px] font-650 tracking-[-0.05em] text-ink">
              {greetingName ? t('login.titleNamed', { name: greetingName }) : t('login.title')}
            </h1>
            <p className="mb-6 mt-2 text-body leading-[1.5] text-ink-3">{t('login.lead')}</p>
            <Form {...unlockForm}>
              <form
                onSubmit={unlockForm.handleSubmit(submitUnlock)}
                aria-busy={unlockPending}
                className="space-y-4"
              >
                <FormField
                  control={unlockForm.control}
                  name="password"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('login.password')}</FormLabel>
                      <FormControl>
                        <PasswordInput
                          autoFocus
                          autoComplete="current-password"
                          readOnly={unlockPending}
                          {...field}
                        />
                      </FormControl>
                      {!unlockPending && <FormMessage />}
                    </FormItem>
                  )}
                />
                <Button
                  type="submit"
                  className="h-10 w-full text-body"
                  disabled={unlockPending}
                  loading={unlockPending}
                >
                  {unlockPending ? t('login.unlocking') : t('login.unlock')}
                </Button>
                <div aria-live="polite" className="empty:hidden">
                  {unlockPending && (
                    <p className="m-0 flex items-center gap-2 text-caption font-500 text-ink-3">
                      <RefreshCw className="size-[13px] shrink-0 animate-spin" aria-hidden />
                      {t('login.decrypting')}
                    </p>
                  )}
                </div>
              </form>
            </Form>
            <div className="mt-[22px] flex items-center justify-between border-t border-line-soft pt-4 text-micro font-500 text-ink-4">
              <span>
                {lockedAt !== null
                  ? lockedToday
                    ? t('lock.lockedToday', { time: fmt.time(lockedAt / 1000) })
                    : t('lock.lockedOn', { date: fmt.dateTime(lockedAt / 1000) })
                  : t('lock.encrypted')}
              </span>
              <button
                type="button"
                onClick={() => setPanel('recover')}
                disabled={unlockPending}
                className="font-600 text-ink-3 underline-offset-[3px] hover:text-ink hover:underline disabled:pointer-events-none disabled:opacity-50"
              >
                {t('login.forgotPassword')}
              </button>
            </div>
          </>
        )}

        {showPanel === 'recover' && (
          <>
            <RecoverySteps current={recoverPending ? 2 : 1} />
            <h1 className="m-0 text-[24px] font-650 tracking-[-0.05em] text-ink">
              {t('recover.title')}
            </h1>
            <p className="mb-6 mt-2 text-body leading-[1.5] text-ink-3">{t('recover.lead')}</p>
            <Form {...recoverForm}>
              <form
                onSubmit={recoverForm.handleSubmit(submitRecover)}
                aria-busy={recoverPending}
                className="space-y-4"
              >
                <FormField
                  control={recoverForm.control}
                  name="recoveryKey"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('recover.recoveryKey')}</FormLabel>
                      <FormControl>
                        <Input
                          placeholder="XXXXXX-XXXXXX-XXXXXX-XXXXXX"
                          autoFocus
                          autoComplete="off"
                          className="uppercase tracking-[0.06em] num"
                          readOnly={recoverPending}
                          {...field}
                        />
                      </FormControl>
                      <FormDescription>{t('recover.keyHint')}</FormDescription>
                      {!recoverPending && <FormMessage />}
                    </FormItem>
                  )}
                />
                <div className="grid grid-cols-2 gap-3">
                  <FormField
                    control={recoverForm.control}
                    name="newPassword"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('recover.newPassword')}</FormLabel>
                        <FormControl>
                          <PasswordInput
                            placeholder={t('recover.newPasswordPlaceholder')}
                            autoComplete="new-password"
                            {...field}
                          />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={recoverForm.control}
                    name="confirmPassword"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('recover.confirmPassword')}</FormLabel>
                        <FormControl>
                          <PasswordInput autoComplete="new-password" {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                </div>
                <Button
                  type="submit"
                  className="h-10 w-full text-body"
                  disabled={recoverPending}
                  loading={recoverPending}
                >
                  {recoverPending ? t('recover.recovering') : t('recover.recoverAccount')}
                </Button>
              </form>
            </Form>
            <div className="mt-[22px] flex items-center justify-between border-t border-line-soft pt-4 text-micro font-500 text-ink-4">
              <span className="flex items-center gap-1.5">
                {recoverPending && (
                  <RefreshCw className="size-[13px] shrink-0 animate-spin" aria-hidden />
                )}
                {recoverPending ? t('recover.verifying') : t('recover.takesSeconds')}
              </span>
              <button
                type="button"
                onClick={() => setPanel('unlock')}
                disabled={recoverPending}
                className="font-600 text-ink-3 underline-offset-[3px] hover:text-ink hover:underline disabled:pointer-events-none disabled:opacity-50"
              >
                {t('recover.backToLogin')}
              </button>
            </div>
          </>
        )}

        {showPanel === 'newKey' && recoveryKey && (
          <>
            <RecoverySteps current={3} />
            <h1 className="m-0 text-[24px] font-650 tracking-[-0.05em] text-ink">
              {t('recover.newKeyTitle')}
            </h1>
            <p className="mb-6 mt-2 text-body leading-[1.5] text-ink-3">
              {t('recover.newKeyLead')}
            </p>
            <RecoveryKeyBox recoveryKey={recoveryKey} />
            <label className="mt-4 flex cursor-pointer items-start gap-[9px] text-table font-500 text-ink-2">
              <Checkbox
                className="mt-0.5"
                checked={keySaved}
                onCheckedChange={(v) => setKeySaved(v === true)}
              />
              {t('recoveryKey.savedElsewhere')}
            </label>
            <p className="mb-4 mt-3 text-micro font-500 text-ink-4">
              {t('recoveryKey.folderNote')}
            </p>
            <Button
              className="h-10 w-full text-body"
              disabled={!keySaved || confirmRecoveryMutation.isPending}
              loading={confirmRecoveryMutation.isPending}
              onClick={() =>
                confirmRecoveryMutation.mutate(undefined, {
                  onSuccess: () => {
                    clearRecoveryKey();
                    setKeySaved(false);
                    setPanel('unlock');
                  },
                })
              }
            >
              {confirmRecoveryMutation.isPending
                ? t('recover.recovering')
                : t('recover.continueToApp')}
            </Button>
            <div className="mt-[22px] flex items-center justify-between border-t border-line-soft pt-4 text-micro font-500 text-ink-4">
              <span className="flex items-center gap-1.5">
                <CircleAlert className="size-3 shrink-0" aria-hidden />
                {t('recoveryKey.shownOnce')}
              </span>
              <button
                type="button"
                onClick={() => {
                  clearRecoveryKey();
                  setKeySaved(false);
                  setPanel('unlock');
                }}
                disabled={confirmRecoveryMutation.isPending}
                className="font-600 text-ink-3 underline-offset-[3px] hover:text-ink hover:underline disabled:pointer-events-none disabled:opacity-50"
              >
                {tc('buttons.cancel')}
              </button>
            </div>
          </>
        )}
      </Card>

      <div className="absolute inset-x-0 bottom-5 text-center text-micro font-500 text-ink-5">
        {version ? t('lock.meta', { version }) : t('lock.metaNoVersion')}
      </div>
    </div>
  );
}
