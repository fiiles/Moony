import { useEffect, useRef, useState, type RefObject } from 'react';
import { useFormat } from '@/lib/use-format';
import { dateAxisTicks, type AxisTick } from '@/utils/stock-performance';
import { cn } from '@/lib/utils';

/** A label closer than this to the left edge, or to the "last" label, is dropped. */
const EDGE_LEFT = 14;
const EDGE_RIGHT = 54;
/** Rough width of one label character (10 px type), and the air kept between two labels. */
const CHAR_WIDTH = 5.8;
const LABEL_GAP = 8;

/** Width of an element, kept up to date by a ResizeObserver (0 until the first measurement). */
function useElementWidth(ref: RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setWidth(entry.contentRect.width);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref]);
  return width;
}

export interface DateAxisProps {
  /** Unix seconds of the first and last point of the chart above. */
  t0: number;
  t1: number;
  /** Width of the value-tick gutter on the right; the time axis stops before it. */
  gutter: number;
  /** Label at the right end of the axis: "Dnes" or the end date. */
  lastLabel: string;
  className?: string;
}

/**
 * Date axis under a performance chart (design system §8): labels at their real positions
 * (1st and 15th of a month, month starts, quarter starts or years, by the span) with a 5 px
 * tick mark each, and `lastLabel` right-aligned at the end of the plot. January 1 reads bold.
 */
export function DateAxis({ t0, t1, gutter, lastLabel, className }: DateAxisProps) {
  const fmt = useFormat();
  const ref = useRef<HTMLDivElement>(null);
  const width = useElementWidth(ref);
  const plot = width - gutter;
  const span = t1 - t0;

  // "led 26": some locales (cs) spell the month out when a year is added to `month: 'short'`
  const monthYear = (t: number): string => {
    const combined = fmt.day(t, { month: 'short', year: '2-digit' });
    if (!combined.startsWith(fmt.day(t, { month: 'long' }))) return combined;
    return `${fmt.day(t, { month: 'short' })} ${fmt.day(t, { year: '2-digit' })}`;
  };

  const label = (tick: AxisTick): string => {
    switch (tick.kind) {
      case 'day':
        return fmt.day(tick.t, { day: 'numeric', month: 'numeric' });
      case 'month':
        return tick.yearStart ? monthYear(tick.t) : fmt.day(tick.t, { month: 'short' });
      case 'quarter':
        return monthYear(tick.t);
      case 'year':
        return fmt.day(tick.t, { year: 'numeric' });
    }
  };

  // Drop ticks whose label would crowd an edge or the previous one (needs the measured width)
  const labelled = dateAxisTicks(t0, t1).map((tick) => ({ tick, text: label(tick) }));
  let previous = { x: -Infinity, width: 0 };
  const visible = labelled.filter(({ tick, text }) => {
    if (plot <= 0) return true;
    const x = ((tick.t - t0) / span) * plot;
    const labelWidth = text.length * CHAR_WIDTH;
    if (x < EDGE_LEFT || x > plot - EDGE_RIGHT) return false;
    if (x - previous.x < (previous.width + labelWidth) / 2 + LABEL_GAP) return false;
    previous = { x, width: labelWidth };
    return true;
  });
  const left = (t: number) => `calc((100% - ${gutter}px) * ${(t - t0) / span})`;

  return (
    <div
      ref={ref}
      aria-hidden
      className={cn('relative h-[22px] text-micro font-500 text-chart-axis', className)}
    >
      {visible.map(({ tick, text }) => (
        <span
          key={tick.t}
          className="absolute top-0 -translate-x-1/2 whitespace-nowrap"
          style={{ left: left(tick.t) }}
        >
          <i className="mx-auto block h-[5px] w-px bg-chart-axis opacity-70" />
          <span className={cn('mt-px block', tick.yearStart && 'font-650 text-ink-3')}>{text}</span>
        </span>
      ))}
      <span
        className="absolute top-1.5 -translate-x-full whitespace-nowrap font-600 text-ink-3"
        style={{ left: `calc(100% - ${gutter}px)` }}
      >
        {lastLabel}
      </span>
    </div>
  );
}
