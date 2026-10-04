import { cn } from '@/lib/utils';

interface RangeBarProps {
  low: number;
  high: number;
  current: number;
  target?: number | null;
  className?: string;
  /** Accessible description, e.g. "52T rozpětí 86,62 – 184,48, cena 182,40". */
  label?: string;
}

/**
 * 52-week range bar (design system, `.range`): a 2 px track, the current price
 * as a dark dot with a paper halo, the target as a green tick.
 */
export function RangeBar({ low, high, current, target, className, label }: RangeBarProps) {
  const span = high - low;
  const pos = (v: number) => (span > 0 ? Math.max(0, Math.min(100, ((v - low) / span) * 100)) : 50);
  return (
    <span
      role={label ? 'img' : undefined}
      aria-label={label}
      className={cn('relative inline-block h-3 w-[92px] shrink-0 align-middle', className)}
    >
      <i className="absolute inset-x-0 top-[5px] h-0.5 rounded-px bg-well-3" />
      {target !== null && target !== undefined && isFinite(target) && (
        <i
          className="absolute top-px h-[10px] w-0.5 -translate-x-1/2 rounded-px bg-gain"
          style={{ left: `${pos(target)}%` }}
        />
      )}
      <i
        className="absolute top-0.5 size-2 -translate-x-1/2 rounded-full bg-chart-line shadow-[0_0_0_2px_var(--paper)]"
        style={{ left: `${pos(current)}%` }}
      />
    </span>
  );
}

/** The bar between its two bounds, as the table and the stat show it. */
export function RangeWithLabels({
  lowLabel,
  highLabel,
  className,
  ...bar
}: RangeBarProps & { lowLabel: string; highLabel: string }) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-2 text-[10px] font-600 text-ink-4 num',
        className
      )}
    >
      <span>{lowLabel}</span>
      <RangeBar {...bar} />
      <span>{highLabel}</span>
    </span>
  );
}
