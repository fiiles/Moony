import { useTranslation } from 'react-i18next';
import type { StockReportOpenLot } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useReportMoney } from '@/lib/use-report-money';

/** "Z otevřených pozic za X už testem prošlo Y % (Z)."; empty when no open lot has a value. */
export function useShareLine(
  groups: { openValue: number; passedValue: number },
  currency: string
): string {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const { money } = useReportMoney(currency);
  const total = groups.openValue + groups.passedValue;
  if (total <= 0) return '';
  return t('upcoming.footShare', {
    total: money(total),
    percent: fmt.percent(groups.passedValue / total, 0),
    passed: money(groups.passedValue),
  });
}

/** The name of an open lot as the report names it: the company, else the ticker. */
export function lotName(lot: StockReportOpenLot): string {
  return lot.companyName || lot.ticker;
}
