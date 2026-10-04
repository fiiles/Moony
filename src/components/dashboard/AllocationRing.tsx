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
  className?: string;
}

const SERIES = ['var(--s1)', 'var(--s2)', 'var(--s3)', 'var(--s4)'];
const OTHER = 'var(--s-other)';

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
  className,
}: AllocationRingProps) {
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
    <div className={cn('grid grid-cols-[156px_1fr] items-center gap-3', className)}>
      <div
        className="relative size-[130px] rounded-full after:absolute after:inset-[22px] after:rounded-full after:bg-paper after:content-['']"
        style={{ background: gradient }}
        role="img"
        aria-label={slices.map((s) => `${s.label} ${formatPercent(s.percent / 100)}`).join(', ')}
      >
        <div className="absolute inset-0 z-[1] grid place-content-center text-center text-[9px] font-600 text-ink-4">
          {centerLabel}
          <b className="mt-[3px] block text-[14px] tracking-[-0.04em] text-ink num">
            {centerValue}
          </b>
        </div>
      </div>
      <ul className="m-0 grid list-none gap-[11px] p-0">
        {slices.map((s) => (
          <li
            key={s.key}
            className="grid grid-cols-[8px_1fr_auto_auto] items-center gap-[10px] text-caption text-ink-2"
          >
            <i
              aria-hidden
              className={cn('size-[7px] rounded-full', s.other && 'border border-line-strong')}
              style={{ background: s.color }}
            />
            <span>{s.label}</span>
            <span className="text-micro font-500 text-ink-4 num">{formatValue(s.value)}</span>
            <b className="text-caption font-650 text-ink num">{formatPercent(s.percent / 100)}</b>
          </li>
        ))}
      </ul>
    </div>
  );
}
