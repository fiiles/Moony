import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import type { PropertyRow, YearlyBalance } from '@/utils/real-estate';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { propertyStatItems } from '@/mobile/lib/property-detail';

/** Whole amounts: the stats show no decimals. */
const WHOLE = { decimals: 0 } as const;

interface PropertyStatsProps {
  row: PropertyRow;
  balance: YearlyBalance;
  /** The main currency. */
  currency: string;
}

/**
 * The four stats of the property, two rows of two (spec §8.8 "Nemovitost"): Hodnota with its
 * change against the purchase, Vlastní kapitál with the loan to value, Hrubý výnos with the yearly
 * rent and Čistý cashflow of a year — the desktop's numbers (`propertyStatItems`) in the main
 * currency, a missing rate shown as "—" (the market value alone falls back to its own currency,
 * never converted 1:1). Amounts go through the eye; percents stay.
 */
export function PropertyStats({ row, balance, currency }: PropertyStatsProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const items = propertyStatItems(row, balance);
  const money = (value: number, code = currency) => moneyText(value, code, WHOLE);

  const { value, equity, yield: gross, cashflow } = items;
  const appreciation = value.appreciation;
  const valueStat: StatItem = {
    label: tc('labels.value'),
    value:
      value.amount !== null
        ? money(value.amount)
        : money(value.native.amount, value.native.currency),
    note:
      appreciation === null
        ? undefined
        : `${appreciation > 0 ? '▲ ' : appreciation < 0 ? '▼ ' : ''}${tr('detail.vsPurchasePct', {
            percent: fmt.percent(Math.abs(appreciation), 1),
          })}`,
  };

  const equityStat: StatItem = {
    label: tr('detail.heroEquity'),
    value: equity.amount !== null ? money(equity.amount) : '—',
    note: equity.hasLoan
      ? t('property.ltv', { ltv: equity.ltv !== null ? fmt.percent(equity.ltv, 0) : '—' })
      : tr('detail.stats.principalNoLoan'),
  };

  const yieldStat: StatItem = {
    label: tr('detail.stats.grossYield'),
    value: gross.ratio !== null ? fmt.percent(gross.ratio, 1) : '—',
    note: gross.rent
      ? t('property.rentYear', {
          rent:
            gross.rent.main !== null
              ? money(gross.rent.main)
              : money(gross.rent.native.amount, gross.rent.native.currency),
        })
      : tr('detail.stats.noRentNote'),
  };

  const cashflowStat: StatItem = {
    label: tr('detail.stats.cashflow'),
    value:
      cashflow.amount !== null
        ? moneyText(cashflow.amount, currency, { signed: true, ...WHOLE })
        : '—',
    tone: cashflow.tone ?? undefined,
    note: t('property.perYear'),
  };

  return (
    // The two rows sit 10 pt apart, as the stats within a row do
    <div className="[&>:first-child]:mb-2.5">
      <StatRow stats={[valueStat, equityStat]} />
      <StatRow stats={[yieldStat, cashflowStat]} />
    </div>
  );
}
