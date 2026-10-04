import type { ChartEventType } from '@/utils/chart-scale';
import { cn } from '@/lib/utils';

/**
 * Time-trace marks (design system §9): buy ▲ filled with a white ring, sell
 * ▽ outlined, income ○ gain ring, expense/payment ○ dark ring. 12 px, hover
 * or `hot` scales to 1.35. Rendered as the Recharts Scatter `shape`.
 */
export interface EventMarkProps {
  cx: number;
  cy: number;
  type: ChartEventType;
  hot?: boolean;
  /** Number of events folded into this mark; > 1 draws a small count. */
  count?: number;
  onEnter?: () => void;
  onLeave?: () => void;
  onClick?: () => void;
}

export function EventMark({
  cx,
  cy,
  type,
  hot,
  count = 1,
  onEnter,
  onLeave,
  onClick,
}: EventMarkProps) {
  const s = 6;
  const common = {
    className: cn(
      'cursor-pointer transition-transform duration-fast ease-moony [transform-box:fill-box] [transform-origin:center]',
      hot && 'scale-[1.35] drop-shadow-[0_1px_2px_rgba(0,0,0,0.25)]'
    ),
    onMouseEnter: onEnter,
    onMouseLeave: onLeave,
    onClick,
  };
  let glyph: React.ReactNode;
  switch (type) {
    case 'buy':
      glyph = (
        <polygon
          {...common}
          points={`${cx},${cy - s} ${cx + s},${cy + s - 1} ${cx - s},${cy + s - 1}`}
          fill="var(--chart-line)"
          stroke="var(--paper)"
          strokeWidth={2}
        />
      );
      break;
    case 'sell':
      glyph = (
        <polygon
          {...common}
          points={`${cx},${cy + s} ${cx + s},${cy - s + 1} ${cx - s},${cy - s + 1}`}
          fill="var(--paper)"
          stroke="var(--chart-line)"
          strokeWidth={2}
        />
      );
      break;
    case 'income':
      glyph = (
        <circle
          {...common}
          cx={cx}
          cy={cy}
          r={4.5}
          fill="var(--paper)"
          stroke="var(--gain)"
          strokeWidth={2}
        />
      );
      break;
    case 'expense':
    case 'mark':
      glyph = (
        <circle
          {...common}
          cx={cx}
          cy={cy}
          r={4.5}
          fill="var(--paper)"
          stroke="var(--chart-line)"
          strokeWidth={2}
        />
      );
      break;
  }
  return (
    <g>
      {glyph}
      {count > 1 && (
        <text
          x={cx + 8}
          y={cy - 6}
          fontSize={9}
          fontWeight={700}
          fill="var(--ink-3)"
          className="pointer-events-none select-none"
        >
          {count}
        </text>
      )}
    </g>
  );
}
