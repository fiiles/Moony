import { useTranslation } from 'react-i18next';
import { monthsBetween } from '@/utils/duration';

/**
 * Spans in words from a namespace that carries the `duration.years_*`,
 * `duration.months_*` and `duration.days_*` plurals: "7 let 6 měsíců",
 * "29 dní". Whole units only; a span under a month says "0 měsíců".
 */
export function useDurationText(namespace: string) {
  const { t } = useTranslation(namespace);

  const months = (total: number) => {
    const years = Math.floor(total / 12);
    const rest = total % 12;
    const parts: string[] = [];
    if (years > 0) parts.push(t('duration.years', { count: years }));
    if (rest > 0 || parts.length === 0) parts.push(t('duration.months', { count: rest }));
    return parts.join(' ');
  };

  const between = (fromSec: number, toSec: number) =>
    months(monthsBetween(fromSec, toSec).totalMonths);

  const days = (count: number) => t('duration.days', { count });

  /** "za 29 dní" below two months, otherwise in months and years. */
  const ahead = (fromSec: number, toSec: number) => {
    const dayCount = Math.max(0, Math.round((toSec - fromSec) / 86_400));
    return dayCount < 60 ? days(dayCount) : between(fromSec, toSec);
  };

  return { months, between, days, ahead };
}
