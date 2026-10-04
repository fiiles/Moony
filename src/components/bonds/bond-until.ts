import type { TFunction } from 'i18next';
import { monthsBetween } from '@/utils/duration';

/**
 * "za 8 měsíců" / "za 3 roky 2 měsíce" / "do měsíce" for a future day, or
 * "splaceno {{date}}" for a past one. Keys live in the `bonds` namespace.
 */
export function untilText(
  t: TFunction<'bonds'>,
  targetSec: number,
  todaySec: number,
  formatDay: (sec: number) => string
): string {
  if (targetSec < todaySec) return t('until.past', { date: formatDay(targetSec) });
  const { years, months, totalMonths } = monthsBetween(todaySec, targetSec);
  if (totalMonths === 0) return t('until.soon');
  const parts = [
    years > 0 ? t('duration.years', { count: years }) : null,
    months > 0 ? t('duration.months', { count: months }) : null,
  ].filter(Boolean);
  return t('until.in', { duration: parts.join(' ') });
}
