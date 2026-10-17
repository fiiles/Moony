import { useMemo } from 'react';
import { allocationSlices, type AllocationSegment } from '@/utils/allocation-percent';

/** 110 pt ring (spec §10.4), 22 pt thick (prototype `.ring-m`). */
const SIZE = 110;
const STROKE = 22;
const RADIUS = (SIZE - STROKE) / 2;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

const SERIES = ['var(--s1)', 'var(--s2)', 'var(--s3)', 'var(--s4)'];
const OTHER = 'var(--s-other)';

interface AllocationRingProps {
  segments: AllocationSegment[];
  /** Label inside the ring ("Aktiva"). */
  centerLabel: string;
  /** Label of the folded bucket for the fifth and later classes ("Ostatní"). */
  otherLabel: string;
  formatPercent: (ratio: number) => string;
  /** Amounts for the accessible name (the legend shows the shares). */
  formatValue: (value: number) => string;
}

/**
 * Rozložení majetku on the phone (spec §8.3 item 5, prototype frame 02): a 110 pt ring of stroked
 * circles in `s1`–`s4` + `s-other` (at most four classes and Ostatní, as on the desktop ring) on
 * the left, the legend with name and share on the right — never colour alone.
 */
export function AllocationRing({
  segments,
  centerLabel,
  otherLabel,
  formatPercent,
  formatValue,
}: AllocationRingProps) {
  const { slices, total } = useMemo(
    () => allocationSlices(segments, { otherLabel }),
    [segments, otherLabel]
  );

  const arcs = useMemo(() => {
    let start = 0;
    return slices.map((s) => {
      const length = total > 0 ? (s.value / total) * CIRCUMFERENCE : 0;
      const arc = { key: s.key, color: color(s.series), length, start };
      start += length;
      return arc;
    });
  }, [slices, total]);

  return (
    <div className="grid grid-cols-[110px_minmax(0,1fr)] items-center gap-4">
      <div
        role="img"
        aria-label={slices
          .map((s) => `${s.label} ${formatPercent(s.percent / 100)} (${formatValue(s.value)})`)
          .join(', ')}
        className="relative size-[110px]"
      >
        <svg width={SIZE} height={SIZE} viewBox={`0 0 ${SIZE} ${SIZE}`} className="-rotate-90">
          <circle
            cx={SIZE / 2}
            cy={SIZE / 2}
            r={RADIUS}
            fill="none"
            stroke={OTHER}
            strokeWidth={STROKE}
          />
          {arcs.map((arc) => (
            <circle
              key={arc.key}
              cx={SIZE / 2}
              cy={SIZE / 2}
              r={RADIUS}
              fill="none"
              stroke={arc.color}
              strokeWidth={STROKE}
              strokeDasharray={`${arc.length} ${CIRCUMFERENCE - arc.length}`}
              strokeDashoffset={-arc.start}
            />
          ))}
        </svg>
        <span
          aria-hidden
          className="absolute inset-0 flex items-center justify-center text-m-caption font-semibold text-ink-3"
        >
          {centerLabel}
        </span>
      </div>
      <ul className="m-0 min-w-0 list-none p-0 text-m-sub">
        {slices.map((s) => (
          <li
            key={s.key}
            className="grid grid-cols-[10px_minmax(0,1fr)_auto] items-center gap-2 py-1 text-ink-2"
          >
            <i
              aria-hidden
              className={
                s.series === 'other'
                  ? 'block size-2.5 rounded-[3px] shadow-[inset_0_0_0_1px_var(--line)]'
                  : 'block size-2.5 rounded-[3px]'
              }
              style={{ background: color(s.series) }}
            />
            <b className="min-w-0 truncate font-semibold text-ink">{s.label}</b>
            <span className="num">{formatPercent(s.percent / 100)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function color(series: 0 | 1 | 2 | 3 | 'other'): string {
  return series === 'other' ? OTHER : SERIES[series];
}
