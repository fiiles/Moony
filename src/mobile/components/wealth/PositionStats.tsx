import { useTranslation } from 'react-i18next';
import { priceDecimals } from '@shared/currencies';
import { useFormat } from '@/lib/use-format';
import { costPerUnit, type PositionRowBase, type StockRow } from '@/utils/positions';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { useApprox } from '@/mobile/hooks/use-approx';
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
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const fmt = useFormat();
  const moneyText = useMoneyText();

  const { marketValue, yearlyDividend } = holding;
  const dividend = yearlyDividend !== null && yearlyDividend > 0 ? yearlyDividend : null;
  const percentNote =
    dividend === null
      ? ts('detail.noDividend')
      : ts('detail.dividendNote', {
          percent: fmt.percent(
            marketValue !== null && marketValue > 0 ? dividend / marketValue : 0,
            1
          ),
        });
  return {
    label: t('stat.dividends'),
    value:
      dividend === null
        ? '—'
        : moneyText(dividend, reportCurrency, { ...statMoneyOptions(dividend), signed: false }),
    note:
      dividend !== null && holding.isManualDividend
        ? `${percentNote} · ${ts('detail.manual')}`
        : percentNote,
  };
}

interface PositionStatsProps {
  holding: PositionRowBase;
  reportCurrency: string;
  /** How many sells the position has had (the note of the realized result). */
  sells: number;
  /** The last stat of the second row: a stock's dividends, a coin's holding time. */
  sixth: StatItem;
  /** What the position has received in dividends, in the report currency (a stock's; counted in the realized result). */
  dividendsReceived?: number | null;
}

/**
 * The six stats of a position (spec §8.8), two rows of three: what it is worth in its own
 * currency and when its price was fetched, its price in its own currency with the "≈" conversion
 * (README §13, "Money and currencies"), what is invested; then the
 * unrealized and the realized result and `sixth` (a stock's dividends of a year, see
 * `useDividendStat`). The numbers are the backend's (`toStockRow`, `toCryptoRow`), in the report
 * currency; only the value and the price are the position's own.
 */
export function PositionStats({
  holding,
  reportCurrency,
  sells,
  sixth,
  dividendsReceived = null,
}: PositionStatsProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const approx = useApprox();

  const stockCurrency = holding.originalCurrency;
  const nativePrice = holding.originalCurrentPrice;
  const { totalCost, gainLoss, realizedGain } = holding;

  const statMoney = (amount: number, currency = reportCurrency, signed = false) =>
    moneyText(amount, currency, { ...statMoneyOptions(amount), signed });
  const unitMoney = (amount: number, currency = reportCurrency) =>
    moneyText(amount, currency, { decimals: priceDecimals(amount, currency) });

  // The price as the exchange quotes it, in the stock's own currency, with its conversion as the
  // "≈" note (A6); without a price (never fetched, none set) there is nothing to show
  const priced = nativePrice > 0;
  const price = priced ? unitMoney(nativePrice, stockCurrency) : '—';
  const stamp = priceStamp(holding.fetchedAt, Date.now() / 1000);
  // When the price (and so the value) was fetched
  const stampNote = holding.isManualPrice
    ? ts('detail.manual')
    : stamp.kind === 'today'
      ? t('wealth.position.stats.asOf', { when: fmt.time(stamp.at) })
      : stamp.kind === 'day'
        ? t('wealth.position.stats.asOf', {
            when: fmt.day(stamp.at, shortDayOptions(stamp.at)),
          })
        : ts('summary.notUpdated');

  const average = costPerUnit(totalCost, holding.quantity);

  const first: StatItem[] = [
    {
      label: t('stat.value'),
      value: priced ? statMoney(holding.quantity * nativePrice, stockCurrency) : '—',
      note: stampNote,
    },
    {
      label: t('stat.price'),
      value: price,
      note: priced ? approx(nativePrice, stockCurrency) : undefined,
    },
    {
      label: t('stat.invested'),
      value: totalCost === null ? '—' : statMoney(totalCost),
      note: average === null ? undefined : ts('detail.avgShort', { price: unitMoney(average) }),
    },
  ];
  const second: StatItem[] = [
    {
      label: t('stat.unrealizedGain'),
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
      label: t('stat.realized'),
      value: realizedGain === null ? '—' : statMoney(realizedGain, reportCurrency, true),
      tone: statTone(realizedGain),
      // The desktop's two parts, the dividends first. A third of the row holds two short lines at
      // 360–390 pt and "z toho dividendy 1 234 Kč" already takes both, so with dividends the sells
      // count is left out rather than cut (the ledger's Prodeje filter lists them); "zatím žádný
      // prodej" says nothing once the dividends explain the whole value.
      note:
        dividendsReceived !== null && dividendsReceived > 0
          ? ts('detail.realizedDividends', { amount: statMoney(dividendsReceived) })
          : sells > 0
            ? ts('detail.sells', { count: sells })
            : ts('detail.noSells'),
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
