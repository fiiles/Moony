import { useState } from 'react';
import { RowChip } from '@/mobile/components/EntityRow';
import { useInstrumentLogo } from '@/hooks/use-asset-logos';

/**
 * The chip of a stock or coin row: its logo on white inside the 36 pt circle with a solid hairline, the first letters of
 * the ticker while there is none (and when the image fails). Stock logos come from Yahoo through
 * the backend and the encrypted cache, coin icons are bundled: the webview fetches nothing.
 */
export function InstrumentChip({ ticker, type }: { ticker: string; type: 'stock' | 'crypto' }) {
  const src = useInstrumentLogo(ticker, type);
  const [failed, setFailed] = useState<string | undefined>();
  const letters = ticker.slice(0, 2).toUpperCase();
  return (
    <RowChip>
      {src && failed !== src ? (
        // colour-literal-ok: a logo is drawn on white in both themes (its artwork assumes it)
        <img
          alt=""
          src={src}
          loading="lazy"
          onError={() => setFailed(src)}
          // colour-literal-ok: a logo is drawn on white in both themes (its artwork assumes it)
          className="size-full rounded-full border border-line bg-white object-contain p-[12%]"
        />
      ) : (
        // The monogram in the logo's own circle and hairline, so a list of both reads as one (C3)
        <span className="flex size-full items-center justify-center rounded-full border border-line">
          {letters}
        </span>
      )}
    </RowChip>
  );
}
