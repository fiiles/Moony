import { useTranslation } from 'react-i18next';
import type { Milestone } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import { useFormat } from '@/lib/use-format';
import { useCurrency } from '@/lib/currency';
import { utcDayFloor } from '@/utils/chart-axis';
import { formatNativePrice } from '@/utils/stock-monitor';

/**
 * The sub-line of a milestone row: what happens; the date chip already says when. Shared by the
 * desktop agenda and upkeep rows and the phone's agenda row.
 */
export function useMilestoneSubLine() {
  const { t } = useTranslation('milestones');
  const fmt = useFormat();
  const { currencyCode } = useCurrency();
  const thisYear = new Date(utcDayFloor(Date.now() / 1000) * 1000).getUTCFullYear();
  // Day and month inside the current year, the full date otherwise: "17. 4." for a valuation
  // from this spring, "17. 4. 2024" for one from two years ago.
  const day = (d: number | null) => {
    if (d === null) return '';
    return new Date(d * 1000).getUTCFullYear() === thisYear
      ? fmt.day(d, { day: 'numeric', month: 'numeric' })
      : fmt.day(d);
  };
  const money = (m: Milestone) =>
    m.amount === null ? '' : fmt.money(Number(m.amount), recordCurrency(m.currency));
  const price = (m: Milestone, value: string | null) =>
    value === null ? '' : formatNativePrice(Number(value), m.currency, fmt.locale, currencyCode);

  return (m: Milestone): string => {
    switch (m.kind) {
      case 'insurance_anniversary':
        return m.tone === 'info'
          ? t('sub.insurance_anniversary_passed')
          : t('sub.insurance_anniversary', { date: day(m.dueDay) });
      case 'insurance_payment':
      case 'bond_coupon':
      case 'bond_maturity':
        return t(`sub.${m.kind}`, { amount: money(m) });
      case 'watch_target':
        return t(m.direction === 'below' ? 'sub.watch_target_below' : 'sub.watch_target_above', {
          price: price(m, m.referenceAmount),
          target: price(m, m.amount),
        });
      case 'backup_stale':
        return m.sinceDay === null
          ? t('sub.backup_never')
          : t('sub.backup_stale', { date: day(m.sinceDay) });
      case 'balances_stale':
        return t('sub.balances_stale', { count: m.count ?? 0 });
      case 'valuation_stale':
      case 'loan_balance_check':
        return t(`sub.${m.kind}`, { date: day(m.sinceDay) });
      case 'loan_fixation_expired':
        return t('sub.loan_fixation_expired', { date: day(m.dueDay) });
      default:
        return t(`sub.${m.kind}`);
    }
  };
}
