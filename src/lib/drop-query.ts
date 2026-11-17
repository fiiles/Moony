import type { QueryClient, QueryKey } from '@tanstack/react-query';

/**
 * Drops a query whose data a mounted screen shows, so the screen sees it gone. `removeQueries`
 * alone takes the entry out of the cache without telling its observers: the screen keeps the last
 * data until something else re-renders it (owner's report 2026-11-17, "Začít znovu" after three
 * wrong pairing codes showed the code field again with one attempt left). The query is reset first,
 * which tells its observers there is no data and no error, then removed.
 */
export function dropQuery(client: QueryClient, queryKey: QueryKey): void {
  client.getQueryCache().find({ queryKey, exact: true })?.reset();
  client.removeQueries({ queryKey, exact: true });
}
