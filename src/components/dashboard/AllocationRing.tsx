import { useMemo, type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { allocationPercents } from '@/utils/allocation-percent';

export interface AllocationSegment {
  key: string;
  label: string;
  value: number;
}

interface AllocationRingProps {
  segments: AllocationSegment[];
  /** Label inside the ring ("Aktiva"). */
  centerLabel: ReactNode;
  /** Compact total inside the ring ("3,20 mil."). */
  centerValue: ReactNode;
  /** Label of the folded bucket for the fifth and later classes. */
  otherLabel: string;
  /** A bucket that always renders in the "other" shade (e.g. "Bez štítku"). */
  otherSegment?: AllocationSegment;
  formatValue: (value: number) => string;
  formatPercent: (ratio: number) => string;
  /**
   * `lg` puts a 184 px ring beside its legend and fills a card that stretches to a taller
   * neighbor: "Poslední pohyby" on the dashboard, the TWR chart on the stocks analysis (wide
   * windows only). `md` is the narrow card: a 140 px ring above a full-width legend.
   */
  size?: 'md' | 'lg';
  className?: string;
}

const SERIES = ['var(--s1)', 'var(--s2)', 'var(--s3)', 'var(--s4)'];
const OTHER = 'var(--s-other)';

const SIZES = {
  // Narrow cards: the legend goes under the ring and gets the whole width (beside it, amounts of
  // seven digits push the percentages out of the card)
  md: {
    root: 'justify-items-center gap-4',
    ring: 'size-[140px] after:inset-[24px]',
    centerLabel: 'text-[9px]',
    centerValue: 'text-[15px]',
    list: 'w-full [&>li]:border-b [&>li]:border-line-soft [&>li]:py-2 [&>li:last-child]:border-0',
    row: 'text-table',
    amount: 'text-caption',
    percent: 'text-table',
  },
  lg: {
    root: 'grid-cols-[184px_1fr] gap-9',
    ring: 'size-[184px] after:inset-[28px]',
    centerLabel: 'text-micro',
    centerValue: 'text-[19px]',
    // Rows divided like the "Poslední pohyby" list next to it
    list: '[&>li]:border-b [&>li]:border-line-soft [&>li]:py-[10px] [&>li:last-child]:border-0',
    row: 'text-table',
    amount: 'text-caption',
    percent: 'text-table',
  },
} as const;

/**
 * Allocation ring (design system §8): a conic ring in s1…s4 plus "Ostatní",
 * legend with amounts and percentages — never color alone. Segments sort by
 * size; the fifth and later fold into the other bucket.
 */
export function AllocationRing({
  segments,
  centerLabel,
  centerValue,
  otherLabel,
  otherSegment,
  formatValue,
  formatPercent,
  size = 'md',
  className,
}: AllocationRingProps) {
  const sz = SIZES[size];
  const { slices, total } = useMemo(() => {
    const positive = segments.filter((s) => s.value > 0).sort((a, b) => b.value - a.value);
    const extra = otherSegment && otherSegment.value > 0 ? otherSegment : null;
    const total = positive.reduce((sum, s) => sum + s.value, 0) + (extra?.value ?? 0);
    const head = positive.slice(0, 4).map((s, i) => ({ ...s, color: SERIES[i], other: false }));
    const rest = positive.slice(4);
    const otherValue = rest.reduce((sum, s) => sum + s.value, 0) + (extra?.value ?? 0);
    const slices =
      otherValue > 0
        ? [
            ...head,
            {
              key: 'other',
              label: rest.length > 0 || !extra ? otherLabel : extra.label,
              value: otherValue,
              color: OTHER,
              other: true,
            },
          ]
        : head;
    // Largest-remainder rounding so the legend adds up to exactly 100 %
    const percents = allocationPercents(slices.map((s) => s.value));
    return { slices: slices.map((s, i) => ({ ...s, percent: percents[i] })), total };
  }, [segments, otherLabel, otherSegment]);

  const gradient = useMemo(() => {
    if (total <= 0) return `conic-gradient(${OTHER} 0% 100%)`;
    let acc = 0;
    const stops = slices.map((s) => {
      const from = acc;
      acc += (s.value / total) * 100;
      return `${s.color} ${from}% ${acc}%`;
    });
    return `conic-gradient(${stops.join(', ')})`;
  }, [slices, total]);

  return (
    <div className={cn('grid items-center', sz.root, className)}>
      <div
        className={cn(
          "relative rounded-full after:absolute after:rounded-full after:bg-paper after:content-['']",
          sz.ring
        )}
        style={{ background: gradient }}
        role="img"
        aria-label={slices.map((s) => `${s.label} ${formatPercent(s.percent / 100)}`).join(', ')}
      >
        <div
          className={cn(
            'absolute inset-0 z-[1] grid place-content-center text-center font-600 text-ink-4',
            sz.centerLabel
          )}
        >
          {centerLabel}
          <b className={cn('mt-[3px] block tracking-[-0.04em] text-ink num', sz.centerValue)}>
            {centerValue}
          </b>
        </div>
      </div>
      <ul className={cn('m-0 grid list-none p-0', sz.list)}>
        {slices.map((s) => (
          <li
            key={s.key}
            className={cn(
              'grid grid-cols-[8px_1fr_auto_auto] items-center gap-[10px] text-ink-2',
              sz.row
            )}
          >
            <i
              aria-hidden
              className={cn('size-[7px] rounded-full', s.other && 'border border-line-strong')}
              style={{ background: s.color }}
            />
            <span>{s.label}</span>
            <span className={cn('font-500 text-ink-4 num', sz.amount)}>{formatValue(s.value)}</span>
            <b className={cn('font-650 text-ink num', sz.percent)}>
              {formatPercent(s.percent / 100)}
            </b>
          </li>
        ))}
      </ul>
    </div>
  );
}
