import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';
import type { ChartEventType } from '@/utils/chart-scale';

type Swatch =
  | { kind: 'line'; color?: string }
  | { kind: 'dash'; color?: string }
  | { kind: 'block'; color: string }
  | { kind: 'dot'; color: string }
  /** White ring with the line color: a milestone on a projection. */
  | { kind: 'ring' }
  | { kind: 'event'; type: ChartEventType };

export interface LegendItem {
  label: ReactNode;
  swatch: Swatch;
}

function SwatchMark({ swatch }: { swatch: Swatch }) {
  switch (swatch.kind) {
    case 'line':
      return (
        <i
          className="block h-0.5 w-3.5 rounded-px bg-chart-line"
          style={swatch.color ? { background: swatch.color } : undefined}
        />
      );
    case 'dash': {
      const color = swatch.color ?? 'var(--chart-cost)';
      return (
        <i
          className="block h-0.5 w-3.5 rounded-px"
          style={{
            background: `repeating-linear-gradient(90deg, ${color} 0 3px, transparent 3px 5px)`,
          }}
        />
      );
    }
    case 'block':
      return <i className="block size-2 rounded-[2px]" style={{ background: swatch.color }} />;
    case 'dot':
      return <i className="block size-[7px] rounded-full" style={{ background: swatch.color }} />;
    case 'ring':
      return (
        <i className="block size-[7px] rounded-full border-[1.5px] border-chart-line bg-paper" />
      );
    case 'event':
      return <EventSwatch type={swatch.type} />;
  }
}

/** Legend glyphs for the time trace (README §9). */
export function EventSwatch({ type }: { type: ChartEventType }) {
  switch (type) {
    case 'buy':
      return (
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
          <polygon points="5,0 10,10 0,10" fill="var(--chart-line)" />
        </svg>
      );
    case 'sell':
      return (
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
          <polygon
            points="0.75,0.75 9.25,0.75 5,9.25"
            fill="var(--paper)"
            stroke="var(--chart-line)"
            strokeWidth="1.5"
          />
        </svg>
      );
    case 'income':
      return (
        <i className="block size-2 rounded-full border-[1.5px] border-gain bg-paper" aria-hidden />
      );
    case 'expense':
    case 'mark':
      return (
        <i
          className="block size-2 rounded-full border-[1.5px] border-chart-line bg-paper"
          aria-hidden
        />
      );
  }
}

/** `.legend`: 10/600 items with a swatch, under the chart; `note` is the quiet sentence at the end. */
export function ChartLegend({
  items,
  note,
  className,
}: {
  items: LegendItem[];
  note?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn('mt-3 flex flex-wrap gap-4 text-micro font-600 text-ink-3', className)}>
      {items.map((item, i) => (
        <span key={i} className="inline-flex items-center gap-1.5">
          <SwatchMark swatch={item.swatch} />
          {item.label}
        </span>
      ))}
      {note && <span className="font-500 text-ink-4">{note}</span>}
    </div>
  );
}
