import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { RefreshCw } from 'lucide-react';
import { priceDecimals } from '@shared/currencies';
import type { StockCompanyInfo } from '@shared/schema';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { formatMarketCap } from '@/utils/stock-monitor';
import { Button } from '@/components/ui/button';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';

interface CompanyCardProps {
  /** The position's name, as the app shows it. */
  name: string;
  /** What Yahoo reported; `undefined` until the first answer. */
  info: StockCompanyInfo | undefined;
  /** The currency the position's price is in, until Yahoo names the listing currency. */
  stockCurrency: string;
  /** The company data has not answered yet (loading, or a retry on its way). */
  pending: boolean;
  onRetry: () => void;
}

/** A figure of the wire (decimal TEXT) as text, `null` when it is missing or not a number. */
function figureText(raw: string | null | undefined, format: (n: number) => string): string | null {
  const n = raw ? Number(raw) : NaN;
  return Number.isFinite(n) ? format(n) : null;
}

/** The quiet sentence of a card that has nothing from Yahoo to show. */
const NOTE = 'm-0 px-1 py-3 text-m-sub leading-[1.5] text-ink-3';

/**
 * "O společnosti" (spec §8.8, prototype frame 12): a key-value card at the end of the detail. The
 * name and the currency are the app's own; the rest is only what Yahoo reported, rows without a
 * figure left out. When it reported nothing, the card says why, as the desktop does: a skeleton
 * while the data is still coming, a note for an ETF (Yahoo reports no company figures for them), a
 * note for a ticker Yahoo knows nothing about, and "could not be loaded" with a retry otherwise.
 * The prices are the stock's own, so they go through the eye like every price; the market cap is
 * the company's size, not an amount of the user's.
 */
export function CompanyCard({ name, info, stockCurrency, pending, onRetry }: CompanyCardProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { currencyCode } = useCurrency();

  // The stored figures are in the listing currency, which differs from the active price's currency
  // when that price was set by hand
  const currency = info?.currency ?? stockCurrency;
  const priceText = (n: number) => moneyText(n, currency, { decimals: priceDecimals(n, currency) });
  const low = figureText(info?.fiftyTwoWeekLow, priceText);
  const high = figureText(info?.fiftyTwoWeekHigh, priceText);

  const profile = [info?.sector, info?.industry].filter(Boolean).join(' · ');
  // `stack`: the label over a value that wraps, for the figures too long for the row's right half
  // (two prices with their currency do not fit a third of a narrow phone in German)
  const rows: { label: string; value: string | null; stack?: boolean }[] = info
    ? [
        {
          label: ts('detail.companyInfo.peRatio'),
          value: figureText(info.peRatio, (n) => fmt.number(n, { maximumFractionDigits: 1 })),
        },
        {
          label: ts('detail.companyInfo.marketCap'),
          value: formatMarketCap(info.marketCap, currency, fmt.locale, currencyCode),
        },
        {
          label: t('wealth.position.range52'),
          value: low !== null && high !== null ? `${low} – ${high}` : null,
          stack: true,
        },
        {
          label: ts('detail.companyInfo.dividendYield'),
          // A raw fraction: 0.0331 is 3.31 %
          value: figureText(info.dividendYield, (n) => fmt.percent(n, 2)),
        },
      ]
    : [];
  const figures = rows.flatMap((row) => (row.value === null ? [] : [{ ...row, value: row.value }]));
  const hasData = profile !== '' || figures.length > 0;
  // Nothing to show yet and the refresh has not answered: placeholder rows, not "unavailable"
  const skeleton = !hasData && pending;

  let rest: ReactNode;
  if (hasData) {
    rest = (
      <>
        {profile && (
          <KeyValueRow stack label={ts('detail.companyInfo.sectorIndustry')} value={profile} />
        )}
        {figures.map(({ label, value, stack }) => (
          <KeyValueRow key={label} label={label} value={value} stack={stack} />
        ))}
      </>
    );
  } else if (skeleton) {
    rest = [0, 1, 2].map((row) => (
      <div key={row} aria-hidden className="flex min-h-11 items-center justify-between px-1 py-2">
        <div className="skeleton h-3 w-2/5" />
        <div className="skeleton h-3 w-1/5" />
      </div>
    ));
  } else if (info?.quoteType === 'ETF') {
    rest = <p className={NOTE}>{ts('detail.companyInfo.etfNote')}</p>;
  } else if (info?.metadataFetchedAt != null) {
    // Yahoo answered and had nothing for this ticker; it is asked again only after a day, so a
    // retry could not change anything
    rest = <p className={NOTE}>{ts('detail.companyInfo.noData')}</p>;
  } else {
    rest = (
      <div className="px-1 py-3">
        <p className="m-0 text-m-sub leading-[1.5] text-ink-3">
          {ts('detail.companyInfo.unavailable')}
        </p>
        <Button
          variant="secondary"
          className="mt-2.5 h-auto min-h-11 whitespace-normal py-2 text-center rounded-[10px] px-4 text-m-sub [&_svg]:size-4"
          onClick={onRetry}
        >
          <RefreshCw strokeWidth={1.75} aria-hidden />
          {ts('detail.companyInfo.retry')}
        </Button>
      </div>
    );
  }

  return (
    <div
      aria-busy={skeleton}
      className="mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-4 py-1 shadow-e1"
    >
      <KeyValueRow stack label={tc('labels.name')} value={name} />
      <KeyValueRow label={tc('labels.currency')} value={currency} />
      {rest}
    </div>
  );
}
