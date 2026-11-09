import type { QueryClient } from '@tanstack/react-query';

/**
 * The rule-7 ritual for changes that arrive without a local write (another device's changes,
 * a restore, a repair): every cache is stale, so invalidate all of it, record today's snapshot
 * from the new data, then refresh the portfolio history that snapshot belongs to.
 */
export async function refreshAfterRemoteChanges(
  qc: Pick<QueryClient, 'invalidateQueries'>,
  recordSnapshot: () => Promise<unknown>
): Promise<void> {
  await qc.invalidateQueries();
  await recordSnapshot();
  await qc.invalidateQueries({ queryKey: ['portfolio-history'] });
}
