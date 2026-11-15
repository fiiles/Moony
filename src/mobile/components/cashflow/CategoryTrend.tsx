import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { useListText } from '@/mobile/hooks/use-list-text';

interface CategoryTrendProps {
  /** UTC month starts, oldest first. */
  months: readonly number[];
  /** What the category came to in each month, in the report currency. */
  totals: readonly number[];
  /** The months of the period the drilldown shows: their bars are filled, the rest are quiet. */
  selected: (month: number) => boolean;
  /** A whole amount in the report currency, for the accessible name and the largest month. */
  formatValue: (value: number) => string;
}

/** Height of the tallest bar. */
const BAR_HEIGHT = 44;

/**
 * The small trend under the category's hero on the drilldown (audit 2026-11-15 D19): six month
 * bars on one baseline, each named by its short month under it, the shown period's bars in `s1`
 * and the months before it in `s-other`; the largest month's amount stands at the top right so the
 * scale reads without an axis. A month that came out negative (refunds) draws no bar. The whole
 * card is one image for VoiceOver, its name listing month and amount.
 */
export function CategoryTrend({ months, totals, selected, formatValue }: CategoryTrendProps) {
  const { t } = useTranslation('mobile');
  const list = useListText();
  const fmt = useFormat();
  const max = Math.max(0, ...totals);
  const label = (month: number) => fmt.day(month, { month: 'short' });
  return (
    <section className="mb-3.5 rounded-r4 border border-line bg-paper px-4 pb-3 pt-3.5 shadow-e1">
      <div className="mb-2.5 flex items-baseline justify-between gap-3">
        <h2 className="m-0 min-w-0 truncate text-m-sub font-semibold text-ink-3">
          {t('cashflow.trend.title', { count: months.length })}
        </h2>
        {max > 0 && (
          <span className="flex-none whitespace-nowrap text-m-caption text-ink-4 num">
            {t('cashflow.trend.max', { amount: formatValue(max) })}
          </span>
        )}
      </div>
      <div
        role="img"
        aria-label={list(months.map((m, i) => `${label(m)} ${formatValue(totals[i])}`))}
        className="grid gap-2"
        style={{ gridTemplateColumns: `repeat(${months.length}, minmax(0, 1fr))` }}
      >
        {months.map((month, i) => {
          const value = Math.max(0, totals[i]);
          const height = max > 0 ? Math.max(value > 0 ? 3 : 0, (value / max) * BAR_HEIGHT) : 0;
          const on = selected(month);
          return (
            <div key={month} aria-hidden className="flex min-w-0 flex-col items-center">
              <div className="flex w-full items-end justify-center" style={{ height: BAR_HEIGHT }}>
                <span
                  className={cn('block w-full max-w-7 rounded-t-[4px]', on && 'bg-s1')}
                  // The months before the period in the ring's quiet tone, seen in both themes
                  style={{ height, background: on ? undefined : 'var(--s-other)' }}
                />
              </div>
              <span
                className={cn(
                  'mt-1 max-w-full truncate text-m-caption num',
                  on ? 'font-semibold text-ink' : 'text-ink-4'
                )}
              >
                {label(month)}
              </span>
            </div>
          );
        })}
      </div>
    </section>
  );
}
