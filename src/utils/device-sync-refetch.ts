/** While a round is running the status is re-read this often, so a missed event cannot stick. */
export const SYNCING_REFETCH_MS = 3000;

/**
 * Refetch interval of the status query: only while the cached phase is `syncing` (a status event
 * lost to a race with the first fetch or the listener setup would otherwise leave it there);
 * `false` otherwise, which keeps the query cheap.
 */
export function syncStatusRefetchInterval(status: { phase?: string } | undefined): number | false {
  return status?.phase === 'syncing' ? SYNCING_REFETCH_MS : false;
}
