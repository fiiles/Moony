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
 * the backend into the encrypted database), a coin its bundled icon: the image sits on white inside
 * a solid hairline. The initials stay while there is no image and when it fails to load. The
 * webview never fetches an image from the network.
 */
export function AssetLogo({ ticker, type, variant = 'series', className }: AssetLogoProps) {
  if (type === 'initials') {
    return <InitialsMark ticker={ticker} letters={2} variant={variant} className={className} />;
  }
  return <InstrumentMark ticker={ticker} type={type} variant={variant} className={className} />;
}

/**
 * The logo alone, for a detail page's head: nothing at all while the instrument has no image, so
 * a page without a logo keeps its plain title.
 */
export function InstrumentHeadLogo({
  ticker,
  type,
  className,
}: {
  ticker: string;
  type: 'stock' | 'crypto';
  className?: string;
}) {
  const [src, onError] = useLogoSrc(ticker, type);
  if (!src) return null;
  return <LogoImage src={src} onError={onError} className={className} />;
}

/** The instrument's image URL (none while unknown or after it failed to load) and its error handler. */
function useLogoSrc(ticker: string, type: 'stock' | 'crypto'): [string | undefined, () => void] {
  const src = useInstrumentLogo(ticker, type);
  const [failed, setFailed] = useState<string | undefined>();
  return [src && failed !== src ? src : undefined, () => setFailed(src)];
}

function InstrumentMark({
  ticker,
  type,
  variant,
  className,
}: {
  ticker: string;
  type: 'stock' | 'crypto';
  variant: 'series' | 'soft';
  className?: string;
}) {
  const [src, onError] = useLogoSrc(ticker, type);
  if (src) return <LogoImage src={src} onError={onError} className={className} />;
  return (
    <InitialsMark
      ticker={ticker}
      letters={type === 'crypto' ? 3 : 2}
      variant={variant}
      className={className}
    />
  );
}

function LogoImage({
  src,
  onError,
  className,
}: {
  src: string;
  onError: () => void;
  className?: string;
}) {
  return (
    <span
      aria-hidden
      className={cn(
        'grid size-[30px] shrink-0 place-items-center overflow-hidden rounded-full border border-line bg-white',
        className
      )}
    >
      <img
        alt=""
        src={src}
        loading="lazy"
        onError={onError}
        className="size-full object-contain p-[12%]"
      />
    </span>
  );
}

function InitialsMark({
  ticker,
  letters,
  variant,
  className,
}: {
  ticker: string;
  letters: number;
  variant: 'series' | 'soft';
  className?: string;
}) {
  return (
    <span
      aria-hidden
      className={cn(
        'grid size-[30px] shrink-0 place-items-center rounded-full text-micro font-750 shadow-[inset_0_1px_rgba(255,255,255,0.13)]',
        variant === 'soft' ? 'bg-well-2 text-ink-2 shadow-none' : SERIES[seriesIndex(ticker)],
        className
      )}
    >
      {ticker.substring(0, letters).toUpperCase()}
    </span>
  );
}
