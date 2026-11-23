import { useTranslation } from 'react-i18next';
import type { StockReportLedgerRow } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import {
  BODY_ROW,
  EntityCell,
  HEAD_ROW,
  NUM_CELL,
  SubLine,
  TableEmpty,
} from './transactions-table-parts';

/**
 * The prototype's grid, widened for amounts in minor units: under 1240 px the columns tighten so
 * nothing overflows at the 1080 px minimum and the title column takes what is left.
 */
const COLUMNS =
  'grid-cols-[84px_92px_minmax(0,1fr)_56px_112px_120px_88px] gap-x-2.5 max-[1239px]:grid-cols-[78px_86px_minmax(0,1fr)_48px_94px_104px_76px] max-[1239px]:gap-x-2';

/** The time-trace glyphs of README §9; the word always follows the glyph. */
const GLYPH = { buy: '▲', sell: '▽', dividend: '○' } as const;

/**
 * "Všechny transakce" (spec D14): every buy, sale and dividend the filters leave, newest first:
 * day, type, title, quantity, price per share in the trade currency, the amount in the report
 * currency with the rate under it, and the fee in the trade currency. A manual dividend has no
 * quantity or price ("—"). The foot counts the rows and explains the glyphs.
 */
export function LedgerTable({
  rows,
  currency,
}: {
  rows: StockReportLedgerRow[];
  currency: string;
}) {
  const { t } = useTranslation('stockTransactions');

  if (rows.length === 0) {
    return <TableEmpty title={t('ledger.emptyTitle')} body={t('ledger.emptyBody')} />;
  }

  return (
    <div>
      <div className={cn(HEAD_ROW, COLUMNS)}>
        <span>{t('ledger.colDay')}</span>
        <span>{t('ledger.colKind')}</span>
        <span>{t('ledger.colTitle')}</span>
        <span className="text-right">{t('ledger.colQuantity')}</span>
        <span className="text-right">{t('ledger.colPrice')}</span>
        <span className="text-right">{t('ledger.colAmount', { currency })}</span>
        <span className="text-right">{t('ledger.colFee')}</span>
      </div>
      {rows.map((row) => (
        <LedgerRow key={`${row.kind}-${row.id}`} row={row} currency={currency} />
      ))}
      <div className="flex items-center justify-between gap-4 px-[21px] py-3 text-micro font-500 text-ink-4">
        <span>{t('ledger.count', { count: rows.length })}</span>
        <span>{t('ledger.legend')}</span>
      </div>
    </div>
  );
}

function LedgerRow({ row, currency }: { row: StockReportLedgerRow; currency: string }) {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const { money, nativeMoney } = useReportMoney(currency);
  const dividend = row.kind === 'dividend';

  const quantity =
    row.quantity === null ? null : fmt.number(Number(row.quantity), { maximumFractionDigits: 6 });

  let rateLine: string | null = null;
  if (row.currency === currency) rateLine = t('table.noConversion');
  else if (row.rate !== null)
    rateLine = t('ledger.rate', {
      rate: fmt.number(Number(row.rate), { maximumFractionDigits: 3 }),
    });

  return (
    <div className={cn(BODY_ROW, COLUMNS)}>
      <span className="whitespace-nowrap num">{fmt.day(row.day)}</span>
      <span className="inline-flex min-w-0 items-center gap-1.5 font-600 text-ink-2">
        <span
          aria-hidden
          className={cn(
            'shrink-0',
            dividend ? 'text-table font-700 text-gain' : 'text-micro text-chart-line'
          )}
        >
          {GLYPH[row.kind]}
        </span>
        <span className="truncate" title={t(`ledger.${row.kind}`)}>
          {t(`ledger.${row.kind}`)}
        </span>
      </span>
      <EntityCell ticker={row.ticker} name={row.companyName} sub={row.ticker} />
      <span className={cn(NUM_CELL, 'truncate')} title={quantity ?? undefined}>
        {quantity ?? '—'}
      </span>
      <div className={NUM_CELL}>
        {row.price === null ? (
          '—'
        ) : (
          <>
            <span className="whitespace-nowrap">
              {nativeMoney(Number(row.price), row.currency)}
            </span>
            {dividend && <SubLine>{t('ledger.perShareGross')}</SubLine>}
          </>
        )}
      </div>
      <div className={NUM_CELL}>
        {row.amount === null ? (
          '—'
        ) : (
          <>
            <span className="whitespace-nowrap">{money(Number(row.amount))}</span>
            {rateLine && <SubLine>{rateLine}</SubLine>}
          </>
        )}
      </div>
      <span className={cn(NUM_CELL, 'whitespace-nowrap')}>
        {row.fee === null ? '—' : nativeMoney(Number(row.fee), row.currency)}
      </span>
    </div>
  );
}
