/**
 * The queries a pull to refresh reads again (`useRefetchShown`): what the screen shows from the
 * profile's own database. A query that reaches the network is left out — the ticker and coin
 * searches (Yahoo, `search_stock_tickers` / `search_crypto`); prices and rates come in only through
 * their own refresh (a class page's pull, `···` › Aktualizovat ceny), never as a side effect of
 * reading a screen again. Every other query of the app reads local data.
 */
export const NETWORK_QUERY_ROOTS: readonly string[] = ['stock-search', 'crypto-search'];

/** Whether a query with `queryKey` is read again by a pull: a local read, not a network one. */
export function refetchedByPull(queryKey: readonly unknown[]): boolean {
  const root = queryKey[0];
  return !(typeof root === 'string' && NETWORK_QUERY_ROOTS.includes(root));
}
