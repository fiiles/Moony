import { useTranslation } from 'react-i18next';
import type { StockReportAmounts, StockReportSummary } from '@shared/schema';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { holdingLength } from './holding-length';

const COLUMNS = ['proceeds', 'cost', 'result', 'fees', 'resultAfterFees'] as const;

type RowKind = 'key' | 'plain' | 'total';

interface Row {
  id: string;
  title: string;
  note?: string;
  amounts: StockReportAmounts;
  kind: RowKind;
}

/**
 * "Příjmy, nabývací cena a výsledek" (spec D8): a small grid inside the hero, five right-aligned
 * columns. With the holding test on: Podléhá dani (emphasised), Osvobozeno časovým testem and
 * Celkem; with it off one row, Všechny prodeje. The result after fees is signed and coloured by
 * gain or loss. The numeric columns size to their amounts and let their heads wrap, so nothing
 * overflows at the 1080 px minimum width; the first column takes the rest.
 */
export function SummaryTable({
  summary,
  months,
  currency,
}: {
  summary: StockReportSummary;
  /** The holding-test length while the test is on, else `null`. */
  months: number | null;
  currency: string;
}) {
  const { t } = useTranslation('stockTransactions');
  const { money, signedMoney } = useReportMoney(currency);

  const length = months === null ? '' : holdingLength(t, months);

  const rows: Row[] =
    months !== null && summary.taxable && summary.exempt
      ? [
          {
            id: 'taxable',
            title: t('summary.taxable'),
            note: t('summary.taxableNote', { length }),
            amounts: summary.taxable,
            kind: 'key',
          },
          {
            id: 'exempt',
            title: t('summary.exempt'),
            note: t('summary.exemptNote', { length }),
            amounts: summary.exempt,
            kind: 'plain',
          },
          { id: 'total', title: t('summary.total'), amounts: summary.total, kind: 'total' },
        ]
      : [
          {
            id: 'all',
            title: t('summary.all'),
            note: t('summary.allNote'),
            amounts: summary.total,
            kind: 'total',
          },
        ];

  const cell = (row: Row, column: (typeof COLUMNS)[number]) => {
    const value = Number(row.amounts[column]);
    if (column === 'resultAfterFees') {
      return (
        <span className={cn('font-650', value > 0 && 'text-gain', value < 0 && 'text-loss')}>
          {signedMoney(value)}
        </span>
      );
    }
    return column === 'result' ? signedMoney(value) : money(value);
  };

  return (
    <div
      role="table"
      aria-label={t('summary.title')}
      className="grid grid-cols-[minmax(112px,1fr)_repeat(5,auto)] items-center gap-x-3"
    >
      <div role="row" className="contents">
        <span role="columnheader" className="border-b border-line-soft pb-[7px]" />
        {COLUMNS.map((column) => (
          <span
            key={column}
            role="columnheader"
            className="self-stretch border-b border-line-soft pb-[7px] text-right text-thead uppercase leading-[1.35] text-ink-3 flex items-end justify-end"
          >
            {t(`summary.${column}`)}
          </span>
        ))}
      </div>
      {rows.map((row, i) => {
        const last = i === rows.length - 1;
        const border = !last && 'border-b border-line-soft';
        const strong = row.kind !== 'plain';
        return (
          <div key={row.id} role="row" className="contents">
            <div
              role="rowheader"
              className={cn('flex min-h-[46px] min-w-0 flex-col justify-center py-1.5', border)}
            >
              <b className="block text-table font-650 text-ink">{row.title}</b>
              {row.note && (
                <small className="mt-0.5 block text-micro font-500 text-ink-4">{row.note}</small>
              )}
            </div>
            {COLUMNS.map((column) => (
              <span
                key={column}
                role="cell"
                className={cn(
                  'flex min-h-[46px] items-center justify-end self-stretch whitespace-nowrap text-right text-table num',
                  strong ? 'font-650 text-ink' : 'font-480 text-ink-2',
                  border
                )}
              >
                {cell(row, column)}
              </span>
            ))}
          </div>
        );
      })}
    </div>
  );
}
