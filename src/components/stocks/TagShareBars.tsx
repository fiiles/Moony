import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Card } from '@/components/ui/card';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { buildTagShareRows } from '@/utils/tag-share';
import type { TagMetrics } from '@shared/schema';

/**
 * Each tag's share of the portfolio as a horizontal bar, largest first, with the tag name, the
 * value and the percentage written out. Replaces the old 0-100 % stacked chart, whose
 * bars were all but invisible and whose 100 % had no clear meaning.
 */
export function TagShareBars({ metrics }: { metrics: TagMetrics[] }) {
  const { t } = useTranslation('reports');
  const { formatCurrency } = useCurrency();
  const fmt = useFormat();
  const rows = useMemo(() => buildTagShareRows(metrics), [metrics]);

  // Nothing to compare when no tag holds any value
  if (rows.every((row) => row.barPercent === 0)) return null;

  return (
    <Card className="p-6 ">
      <h3 className="text-lg font-semibold">{t('stocksAnalysis.metricsChart')}</h3>
      <p className="mb-4 text-sm text-ink-3">{t('stocksAnalysis.metricsChartDescription')}</p>
      <ul className="space-y-3">
        {rows.map((row) => (
          <li
            key={row.id}
            className="grid grid-cols-[minmax(5rem,12rem)_1fr_auto] items-center gap-3"
          >
            <span className="truncate text-sm font-medium" title={row.name}>
              {row.name}
            </span>
            <div className="h-3 overflow-hidden rounded-full bg-well" aria-hidden="true">
              <div
                className="h-full rounded-full"
                style={{
                  width: `${row.barPercent}%`,
                  minWidth: row.barPercent > 0 ? 4 : 0,
                  backgroundColor: row.color || 'var(--s-other)',
                }}
              />
            </div>
            <span className="min-w-[10rem] whitespace-nowrap text-right text-sm tabular-nums">
              <span className="font-semibold">{fmt.percent(row.percent / 100, 1)}</span>
              <span className="ml-2 text-ink-3">{formatCurrency(row.value)}</span>
            </span>
          </li>
        ))}
      </ul>
    </Card>
  );
}
