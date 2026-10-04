import type { ReactNode } from 'react';
import { tipClassName } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

/**
 * Chart tooltip body (design system §8, `.tip`): dark material, 10 px, a
 * bold first line and muted lines under it. Used by the Recharts `content`
 * prop and by the time-trace markers.
 */
export function ChartTip({
  title,
  lines = [],
  className,
}: {
  title: ReactNode;
  lines?: ReactNode[];
  className?: string;
}) {
  return (
    <div className={cn(tipClassName, 'pointer-events-none whitespace-nowrap', className)}>
      <b>{title}</b>
      {lines.filter(Boolean).map((line, i) => (
        <small key={i}>{line}</small>
      ))}
    </div>
  );
}

/**
 * Positions a ChartTip above a point inside a `relative` chart container. The
 * tip slides along its own width with the point (centred in the middle, flush
 * left at the left edge, flush right at the right edge), so a mark near an
 * edge never pushes it out of a card that clips its content.
 */
export function FloatingTip({
  x,
  y,
  width,
  children,
}: {
  x: number;
  y: number;
  /** Container width, to keep the tip inside. */
  width: number;
  children: ReactNode;
}) {
  const clampedX = Math.max(8, Math.min(width - 8, x));
  const shift = width > 0 ? Math.max(0, Math.min(100, (clampedX / width) * 100)) : 50;
  return (
    <div
      className="pointer-events-none absolute z-[6]"
      style={{ left: clampedX, top: y - 12, transform: `translate(-${shift}%, -100%)` }}
    >
      {children}
    </div>
  );
}
