import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/hooks/use-auth';
import { useLanguage } from '@/i18n/I18nProvider';
import { greetingMessage } from '@/utils/greeting';
import { vocative } from '@/utils/vocative';

/**
 * The overview's title ("Dobrý den, Filipe."), shared by the desktop dashboard and the phone's
 * Přehled. The first visit right after onboarding greets with "Vítejte", later visits by the time
 * of day; the first-run flag is consumed on first read. A profile without a name is greeted without one.
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
  const message = greetingMessage(new Date().getHours(), isFirstVisit, name);
  return t(message.key, { name: message.name });
}
