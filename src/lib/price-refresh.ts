import type { QueryClient } from '@tanstack/react-query';

/**
 * Queries built on current prices (and the rates they convert at): position lists and
 * details, the stocks analysis, the stock performance, the projection, net worth and the value
 * histories, and the spending plans (their spend converts at the payments' days, their totals at
 * the current rates). The query client never goes stale on its own (staleTime: Infinity), so every
 * price refresh invalidates all of them.
 */
export const PRICE_DEPENDENT_KEYS = [
  'investments',
  'investment',
  'crypto',
  'stocks-analysis',
  'stock-performance',
  'projection',
  'portfolio-metrics',
  'portfolio-history',
  'ticker-history',
  'spending-plans',
  'spending-plan',
] as const;

/**
 * One refresh contract for every stock or crypto price refresh. The stock quote refresh also
 * runs the daily split check, which can add split rows, so `['stock-splits']` is refreshed too.
 */
export function invalidateAfterPriceRefresh(client: QueryClient): Promise<void> {
  return Promise.all(
    [...PRICE_DEPENDENT_KEYS, 'stock-splits'].map((key) =>
      client.invalidateQueries({ queryKey: [key] })
    )
  ).then(() => undefined);
}
