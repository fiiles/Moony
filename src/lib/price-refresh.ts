import type { QueryClient } from '@tanstack/react-query';

/**
 * Queries built on current prices (and the rates they convert at): position lists and
 * details, the stocks analysis, tag metrics, TWR, the projection, net worth and the value
 * histories. The query client never goes stale on its own (staleTime: Infinity), so every
 * price refresh invalidates all of them.
 */
export const PRICE_DEPENDENT_KEYS = [
  'investments',
  'investment',
  'crypto',
  'stocks-analysis',
  'tag-metrics',
  'stock-twr',
  'projection',
  'portfolio-metrics',
  'portfolio-history',
  'ticker-history',
] as const;

/** One refresh contract for every stock or crypto price refresh. */
export function invalidateAfterPriceRefresh(client: QueryClient): Promise<void> {
  return Promise.all(
    PRICE_DEPENDENT_KEYS.map((key) => client.invalidateQueries({ queryKey: [key] }))
  ).then(() => undefined);
}
