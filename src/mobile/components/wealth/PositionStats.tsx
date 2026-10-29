import { useTranslation } from 'react-i18next';
import { priceDecimals } from '@shared/currencies';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { costPerUnit, type PositionRowBase, type StockRow } from '@/utils/positions';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { shortDayOptions } from '@/mobile/lib/format-day';
import { priceStamp } from '@/mobile/lib/position-rows';
import { statMoneyOptions, statTone } from '@/mobile/lib/stat-money';

/**
 * The dividends of a year as the sixth stat of a stock's position: the yearly dividend of the
 * position in the report currency with its yield on the value, "—" without one. A hook for the
 * words; the number is the backend's (`toStockRow`).
 */
// eslint-disable-next-line react-refresh/only-export-components -- the stock's sixth stat lives with the stats it completes
export function useDividendStat(holding: StockRow, reportCurrency: string): StatItem {
  const { t: ts } = useTranslation('stocks');
  const fmt = useFormat();
  const moneyText = useMoneyText();

  const { marketValue, yearlyDividend } = holding;
  const dividend = yearlyDividend !== null && yearlyDividend > 0 ? yearlyDividend : null;
  return {
    label: ts('detail.dividends'),
    value:
      dividend === null
        ? '—'
        : moneyText(dividend, reportCurrency, { ...statMoneyOptions(dividend), signed: false }),
    note:
      dividend === null
        ? ts('detail.noDividend')
        : ts('detail.dividendNote', {
            percent: fmt.percent(
              marketValue !== null && marketValue > 0 ? dividend / marketValue : 0,
              1
            ),
          }),
  };
}

interface PositionStatsProps {
  holding: PositionRowBase;
  reportCurrency: string;
  /** How many sells the position has had (the note of the realized result). */
  sells: number;
  /** The last stat of the second row: a stock's dividends, a coin's holding time. */
  sixth: StatItem;
}

/**
 * The six stats of a position (spec §8.8), two rows of three: what it is worth in its own
 * currency, its price in the report currency and when it was fetched, what is invested; then the
 * unrealized and the realized result and `sixth` (a stock's dividends of a year, see
 * `useDividendStat`). The numbers are the backend's (`toStockRow`, `toCryptoRow`), in the report
 * currency; only the value and the price are the position's own.
 */
export function PositionStats({ holding, reportCurrency, sells, sixth }: PositionStatsProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { convert } = useCurrency();

  const stockCurrency = holding.originalCurrency;
  const nativePrice = holding.originalCurrentPrice;
  const { totalCost, gainLoss, realizedGain } = holding;

  const statMoney = (amount: number, currency = reportCurrency, signed = false) =>
    moneyText(amount, currency, { ...statMoneyOptions(amount), signed });
  const unitMoney = (amount: number, currency = reportCurrency) =>
    moneyText(amount, currency, { decimals: priceDecimals(amount, currency) });

  // The price in the report currency; without a rate the stock's own price stands in, without a
  // price (never fetched, none set) there is nothing to show
  const priced = nativePrice > 0;
  const converted = convert(nativePrice, stockCurrency, reportCurrency);
  const price = !priced
    ? '—'
    : converted !== null
      ? unitMoney(converted)
      : unitMoney(nativePrice, stockCurrency);
  const stamp = priceStamp(holding.fetchedAt, Date.now() / 1000);
  const priceNote = holding.isManualPrice
    ? ts('detail.manual')
    : stamp.kind === 'today'
      ? fmt.time(stamp.at)
      : stamp.kind === 'day'
        ? fmt.day(stamp.at, shortDayOptions(stamp.at))
        : ts('summary.notUpdated');

  const average = costPerUnit(totalCost, holding.quantity);

  const first: StatItem[] = [
    {
      label: t('wealth.position.stats.value'),
      value: priced ? statMoney(holding.quantity * nativePrice, stockCurrency) : '—',
      note: t('wealth.position.stats.inCurrency', { currency: stockCurrency }),
    },
    { label: t('wealth.position.stats.price'), value: price, note: priceNote },
    {
      label: ts('detail.invested'),
      value: totalCost === null ? '—' : statMoney(totalCost),
      note: average === null ? undefined : ts('detail.avgShort', { price: unitMoney(average) }),
    },
  ];
  const second: StatItem[] = [
    {
      label: ts('detail.unrealized'),
      value: gainLoss === null ? '—' : statMoney(gainLoss, reportCurrency, true),
      tone: statTone(gainLoss),
      note:
        gainLoss !== null && holding.gainLossPercent !== null
          ? ts('detail.vsInvestedPct', {
              percent: fmt.percent(holding.gainLossPercent / 100, 1, { signed: true }),
            })
          : undefined,
    },
    {
      label: ts('detail.realized'),
      value: realizedGain === null ? '—' : statMoney(realizedGain, reportCurrency, true),
      tone: statTone(realizedGain),
      note: sells > 0 ? ts('detail.sells', { count: sells }) : ts('detail.noSells'),
    },
    sixth,
  ];

  return (
    <>
      <StatRow stats={first} />
      <StatRow stats={second} />
    </>
  );
}
