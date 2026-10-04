import { cn } from '@/lib/utils';

interface AssetLogoProps {
  ticker: string;
  type: 'stock' | 'crypto';
  /** `soft`: the well-2 variant for rows that are not an asset (bank movements). */
  variant?: 'series' | 'soft';
  className?: string;
}

const SERIES = [
  'bg-s1 text-ink-inverse',
  'bg-s2 text-ink-inverse',
  'bg-s3 text-ink-inverse',
  'bg-s4 text-ink',
];

/** Stable series index for a ticker so the same instrument keeps its shade everywhere. */
function seriesIndex(ticker: string): number {
  let h = 0;
  for (let i = 0; i < ticker.length; i++) h = (h * 31 + ticker.charCodeAt(i)) >>> 0;
  return h % SERIES.length;
}

/**
 * Instrument mark (design system §6, `.logo`): a 30 px circle in one of the
 * grey series shades with the ticker's initials. Logos are intentionally NOT
 * fetched from third-party image hosts: that would leak every tracked ticker.
 */
export function AssetLogo({ ticker, type, variant = 'series', className }: AssetLogoProps) {
  const initials = (
    type === 'stock' ? ticker.substring(0, 2) : ticker.substring(0, 3)
  ).toUpperCase();
  return (
    <span
      aria-hidden
      className={cn(
        'grid size-[30px] shrink-0 place-items-center rounded-full text-micro font-750 shadow-[inset_0_1px_rgba(255,255,255,0.13)]',
        variant === 'soft' ? 'bg-well-2 text-ink-2 shadow-none' : SERIES[seriesIndex(ticker)],
        className
      )}
    >
      {initials}
    </span>
  );
}
