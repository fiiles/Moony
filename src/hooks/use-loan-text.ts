import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import { monthsBetween } from '@/utils/duration';

/**
 * Loan wording shared by the list and the detail: "3/2028" for a month, "16 let
 * 6 měsíců" for a span of months (Czech plurals from the loans namespace).
 */
export function useLoanText() {
  const { t } = useTranslation('loans');
  const fmt = useFormat();

  /** "3/2028" — numeric month and year of a UTC day. */
  const monthYear = (unixSeconds: number) =>
    fmt.month(new Date(unixSeconds * 1000), { month: 'numeric', year: 'numeric' });

  /** "16 let 6 měsíců" from a count of months; "0 měsíců" for none. */
  const months = (total: number) => {
    const years = Math.floor(total / 12);
    const rest = total % 12;
    const parts: string[] = [];
    if (years > 0) parts.push(t('duration.years', { count: years }));
    if (rest > 0 || parts.length === 0) parts.push(t('duration.months', { count: rest }));
    return parts.join(' ');
  };

  /** Span between two UTC days in whole months, worded. */
  const between = (fromSec: number, toSec: number) =>
    months(monthsBetween(fromSec, toSec).totalMonths);

  return { monthYear, months, between };
}
