import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import type { StockMonitorDetail } from '@shared/schema';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { exchangeName } from '@/utils/exchange-names';
import { formatMarketCap, percentFromFraction } from '@/utils/stock-monitor';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { backState } from '@/mobile/lib/back-label';
import { priceOf, watchPriceText } from '@/mobile/lib/watch-detail';
import { positionPath } from '@/mobile/shell/routes';

/**
 * "Klíčové údaje" of a watched stock (spec §8.8): the section head and a key-value card (`.m-kvcard`, the company
 * card's look) with the exchange, the currency, the market cap, the P/E, the dividend yield and
 * the 52-week range, rows without a figure left out as the company card does. When the stock is
 * also held, the last row — "V portfoliu" — opens the position, whose back button then names the
 * ticker (back returns here, not to Akcie). The 52-week low and high are
 * Yahoo's public prices like the hero's, so they stay visible while amounts are hidden (ruling
 * P1); the range is stacked because two prices with their currency do not fit a third of a narrow
 * phone in German.
 */
export function WatchKeyData({ detail }: { detail: StockMonitorDetail }) {
  const { t } = useTranslation('mobile');
  const { t: tm } = useTranslation('stockMonitor');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { currencyCode } = useCurrency();
  const [, setLocation] = useLocation();

  const { currency } = detail;
  const low = priceOf(detail.fiftyTwoWeekLow);
  const high = priceOf(detail.fiftyTwoWeekHigh);
  const pe = priceOf(detail.peRatio);
  const dividend = percentFromFraction(detail.trailingDividendYield);

  const rows: { label: string; value: string | null; stack?: boolean }[] = [
    {
      label: tm('detail.kv.exchange'),
      value: detail.exchange ? exchangeName(detail.exchange) : null,
    },
    { label: tc('labels.currency'), value: currency },
    {
      label: tm('detail.kv.marketCap'),
      value: formatMarketCap(detail.marketCap, currency, fmt.locale, currencyCode),
    },
    {
      label: tm('detail.kv.pe'),
      value: pe === null ? null : fmt.number(pe, { maximumFractionDigits: 1 }),
    },
    {
      label: tm('detail.kv.dividend'),
      value: dividend === null ? null : fmt.percent(dividend / 100, 2),
    },
    {
      label: t('wealth.position.range52'),
      value:
        low !== null && high !== null
          ? `${watchPriceText(fmt, low, currency)} – ${watchPriceText(fmt, high, currency)}`
          : null,
      stack: true,
    },
  ];
  const figures = rows.flatMap((row) => (row.value === null ? [] : [{ ...row, value: row.value }]));
  const heldId = detail.isHeld ? detail.heldInvestmentId : null;
  if (figures.length === 0 && heldId === null) return null;

  return (
    <>
      <SectionHead title={tm('detail.keyData')} />
      <div className="mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-4 py-1 shadow-e1">
        {figures.map(({ label, value, stack }) => (
          <KeyValueRow key={label} label={label} value={value} stack={stack} />
        ))}
        {heldId !== null && (
          <KeyValueRow
            label={tm('table.held')}
            value={t('wealth.watch.openPosition')}
            ariaLabel={`${tm('table.held')}, ${tm('actions.openInPortfolio')}`}
            onClick={() => setLocation(positionPath(heldId), { state: backState(detail.ticker) })}
          />
        )}
      </div>
    </>
  );
}
