import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/hooks/use-auth';
import { useLanguage } from '@/i18n/I18nProvider';
import { vocative } from '@/utils/vocative';

/** Greeting by the local hour: morning until 11, day until 18, evening after. */
function greetingKey(hour: number): 'morning' | 'day' | 'evening' {
  if (hour >= 5 && hour < 11) return 'morning';
  if (hour >= 11 && hour < 18) return 'day';
  return 'evening';
}

/**
 * The overview's title ("Dobrý den, Filipe."), shared by the desktop dashboard and the phone's
 * Přehled. The first visit right after onboarding greets with "Vítejte", later visits by the time
 * of day; the first-run flag is consumed on first read.
 */
export function useGreeting(): string {
  const { t } = useTranslation('dashboard');
  const { user } = useAuth();
  const { getLocale } = useLanguage();
  const [isFirstVisit] = useState(() => {
    try {
      const first = localStorage.getItem('moony-first-run') === '1';
      if (first) localStorage.removeItem('moony-first-run');
      return first;
    } catch {
      return false;
    }
  });
  const name = vocative(user?.name ?? '', getLocale());
  return isFirstVisit
    ? t('welcomeFirst', { name })
    : t(`greeting.${greetingKey(new Date().getHours())}`, { name });
}
