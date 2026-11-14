import { useState } from 'react';
import { useInstrumentLogo } from '@/hooks/use-asset-logos';
import { cn } from '@/lib/utils';

interface AssetLogoProps {
  ticker: string;
  /**
   * `stock` and `crypto` take a real ticker and may show its logo. `initials` is for every row that
   * passes a name (loan, bond, policy, property, other asset): two letters, never an image.
   */
  type: 'stock' | 'crypto' | 'initials';
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
 * Instrument mark (design system §6, `.logo`): a 30 px circle in one of the grey series shades with
 * the ticker's initials. A stock shows its logo once the backend has cached it (Yahoo, fetched by
 * the backend into the encrypted database), a coin its bundled icon; the initials stay while there
 * is no image and when it fails to load. The webview never fetches an image from the network.
 */
export function AssetLogo({ ticker, type, variant = 'series', className }: AssetLogoProps) {
  const initials = (
    type === 'crypto' ? ticker.substring(0, 3) : ticker.substring(0, 2)
  ).toUpperCase();
  return (
    <span
      aria-hidden
      className={cn(
        'grid size-[30px] shrink-0 place-items-center overflow-hidden rounded-full text-micro font-750 shadow-[inset_0_1px_rgba(255,255,255,0.13)]',
        variant === 'soft' ? 'bg-well-2 text-ink-2 shadow-none' : SERIES[seriesIndex(ticker)],
        className
      )}
    >
      {type === 'initials' ? (
        initials
      ) : (
        <LogoOrInitials ticker={ticker} type={type} initials={initials} />
      )}
    </span>
  );
}

function LogoOrInitials({
  ticker,
  type,
  initials,
}: {
  ticker: string;
  type: 'stock' | 'crypto';
  initials: string;
}) {
  const src = useInstrumentLogo(ticker, type);
  const [failed, setFailed] = useState<string | undefined>();
  if (!src || failed === src) return <>{initials}</>;
  return (
    <img
      alt=""
      src={src}
      loading="lazy"
      onError={() => setFailed(src)}
      className="size-full rounded-full bg-white object-contain p-[12%]"
    />
  );
}
