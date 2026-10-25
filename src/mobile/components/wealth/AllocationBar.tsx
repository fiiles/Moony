import { useMemo } from 'react';
import { cn } from '@/lib/utils';
import { allocationSlices, type AllocationSegment } from '@/utils/allocation-percent';

const SERIES = ['var(--s1)', 'var(--s2)', 'var(--s3)', 'var(--s4)'];
const OTHER = 'var(--s-other)';

const color = (series: 0 | 1 | 2 | 3 | 'other') => (series === 'other' ? OTHER : SERIES[series]);

interface AllocationBarProps {
  segments: AllocationSegment[];
  /** Label of the folded bucket for the fifth and later classes ("Ostatní"). */
  otherLabel: string;
  formatPercent: (ratio: number) => string;
  /** Amounts for the accessible name (the legend shows the shares); masked by the caller. */
  formatValue: (value: number) => string;
}

/**
 * The thin 100 % allocation bar and its legend (README §13, `.m-abar` and `.m-legend`; spec §8.8):
 * the four largest classes in `s1`–`s4`, the rest folded into "Ostatní" in `s-other`, one span per
 * share in a rounded 8 pt track, and under it a wrapping legend of swatch, name and share — the
 * share is always written, never colour alone. It sits in the hero (`Hero.children`) and counts
 * the hero's total, so it takes every class and not only the rows the menu preference leaves
 * in. Nothing without a positive segment.
 */
export function AllocationBar({
  segments,
  otherLabel,
  formatPercent,
  formatValue,
}: AllocationBarProps) {
  const { slices } = useMemo(
    () => allocationSlices(segments, { otherLabel }),
    [segments, otherLabel]
  );
  if (slices.length === 0) return null;
  return (
    <div className="mt-3">
      <div
        role="img"
        aria-label={slices
          .map((s) => `${s.label} ${formatPercent(s.percent / 100)} (${formatValue(s.value)})`)
          .join(', ')}
        className="flex h-2 overflow-hidden rounded-full bg-well-2"
      >
        {slices.map((s) => (
          <span
            key={s.key}
            className={cn(
              'block h-full min-w-0',
              s.series === 'other' && 'shadow-[inset_0_0_0_1px_var(--line)]'
            )}
            style={{ flexBasis: `${s.percent}%`, background: color(s.series) }}
          />
        ))}
      </div>
      <ul className="m-0 mt-2.5 flex list-none flex-wrap gap-x-3 gap-y-1 p-0 text-m-sub text-ink-2">
        {slices.map((s) => (
          <li key={s.key} className="inline-flex min-w-0 items-center gap-1.5">
            <i
              aria-hidden
              className={cn(
                'block size-2.5 flex-none rounded-[3px]',
                s.series === 'other' && 'shadow-[inset_0_0_0_1px_var(--line)]'
              )}
              style={{ background: color(s.series) }}
            />
            <b className="min-w-0 truncate font-semibold text-ink">{s.label}</b>
            <span className="flex-none num">{formatPercent(s.percent / 100)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
