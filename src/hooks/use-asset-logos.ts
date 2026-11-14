import { useEffect, useState } from 'react';
import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { assetLogosApi } from '@/lib/tauri-api';
import { loadCoinIcon, peekCoinIcon } from '@/utils/coin-icons';

const EMPTY: ReadonlyMap<string, string> = new Map();

/**
 * Upper-case ticker → data URL of every stock logo in the encrypted cache. All rows share the one
 * `['asset-logos']` query. A second query, `['asset-logos-refresh']`, asks the backend once per
 * unlock (the cache is cleared on lock) to look up the logos that are due, and invalidates the
 * first when it stored something.
 */
export function useAssetLogos(): ReadonlyMap<string, string> {
  const queryClient = useQueryClient();
  const { data } = useQuery({
    queryKey: ['asset-logos'],
    queryFn: async () => {
      const logos = await assetLogosApi.getAll();
      return new Map(logos.map((logo) => [logo.ticker.toUpperCase(), logo.dataUrl]));
    },
    staleTime: Infinity,
  });

  useQuery({
    queryKey: ['asset-logos-refresh'],
    queryFn: async () => {
      await refreshAssetLogos(queryClient);
      return null;
    },
    staleTime: Infinity,
    retry: false,
  });

  return data ?? EMPTY;
}

/** Looks up the due logos now (after a stock is added or followed) and reloads the cache if any arrived. Never throws. */
export async function refreshAssetLogos(queryClient: QueryClient): Promise<void> {
  try {
    const stored = await assetLogosApi.refresh();
    if (stored > 0) await queryClient.invalidateQueries({ queryKey: ['asset-logos'] });
  } catch (error) {
    console.error('Asset logo refresh failed:', error);
  }
}

/** Stock data URL or bundled coin icon URL for an instrument; undefined while unknown or absent. */
export function useInstrumentLogo(ticker: string, type: 'stock' | 'crypto'): string | undefined {
  const logos = useAssetLogos();
  // The loaded coin icon, with the ticker it belongs to (a recycled row must not show the old one)
  const [loaded, setLoaded] = useState<{ ticker: string; url: string | null } | null>(null);

  useEffect(() => {
    if (type !== 'crypto' || peekCoinIcon(ticker) !== undefined) return;
    let live = true;
    void loadCoinIcon(ticker).then((url) => {
      if (live) setLoaded({ ticker, url });
    });
    return () => {
      live = false;
    };
  }, [ticker, type]);

  if (type === 'stock') return logos.get(ticker.toUpperCase());
  const known = peekCoinIcon(ticker);
  if (known !== undefined) return known ?? undefined;
  return loaded?.ticker === ticker ? (loaded.url ?? undefined) : undefined;
}
