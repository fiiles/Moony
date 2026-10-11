import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import { axisStamps, isShortSpan } from '@/utils/chart-scale';
import { utcDayFloor } from '@/utils/chart-axis';
import { cn } from '@/lib/utils';

/** Right padding reserved for the value ticks; the time axis stops before it. */
export const AXIS_WIDTH = 54;

interface TimeAxisProps {
  /** Unix seconds of the first and last point. */
  t0: number;
  t1: number;
  /** Number of labels (README §8: five, the last reads "Dnes" when it is today). */
  count?: number;
  /** Width of the value-tick gutter on the right. */
  gutter?: number;
  className?: string;
}

/**
 * Time axis under a chart (design system §8): evenly spaced labels in
 * `chart-axis` 10 px, "2. 10." for short spans, "zář 26" for long ones.
 */
export function TimeAxis({ t0, t1, count = 5, gutter = AXIS_WIDTH, className }: TimeAxisProps) {
  const fmt = useFormat();
  const { t } = useTranslation('common');
  const stamps = axisStamps(t0, t1, count);
  const short = isShortSpan(t0, t1);
  const today = utcDayFloor(Date.now() / 1000);
  const label = (stamp: number, isLast: boolean) => {
    if (isLast && utcDayFloor(stamp) === today) return t('charts.today');
    return short
      ? fmt.day(stamp, { day: 'numeric', month: 'numeric' })
      : fmt.day(stamp, { month: 'short', year: '2-digit' });
  };
  return (
    <div
      aria-hidden
      className={cn('mt-1.5 flex justify-between text-micro font-500 text-chart-axis', className)}
      style={{ paddingRight: gutter }}
    >
      {stamps.map((stamp, i) => (
        <span key={i}>{label(stamp, i === stamps.length - 1)}</span>
      ))}
    </div>
  );
}

/**
 * Category axis under a bar chart: one label per bar, centred under it. `centered` keeps every
 * slot the width of its bar and never wraps a label: one wider than its slot (a month with its
 * year) spreads equally to both sides, which the bars' own labels may rely on when blanks sit
 * next to it.
 */
export function CategoryAxis({
  labels,
  gutter = AXIS_WIDTH,
  centered = false,
  className,
}: {
  labels: string[];
  gutter?: number;
  centered?: boolean;
  className?: string;
}) {
  return (
    <div
      aria-hidden
      className={cn('mt-1.5 flex text-micro font-500 text-chart-axis', className)}
      style={{ paddingRight: gutter }}
    >
      {labels.map((l, i) =>
        centered ? (
          <span key={i} className="flex min-w-0 flex-1 justify-center">
            <span className="shrink-0 whitespace-nowrap">{l}</span>
          </span>
        ) : (
          <span key={i} className="flex-1 text-center">
            {l}
          </span>
        )
      )}
    </div>
  );
}
