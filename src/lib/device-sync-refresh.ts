import type { QueryClient } from '@tanstack/react-query';

/**
 * The rule-7 ritual for changes that arrive without a local write (another device's changes,
 * a restore, a repair): every cache is stale, so invalidate all of it, record today's snapshot
 * from the new data, then refresh the portfolio history that snapshot belongs to.
 *
 * Never rejects: the change already happened, so a failed snapshot is logged and the history
 * refresh still runs.
 */
export async function refreshAfterRemoteChanges(
  qc: Pick<QueryClient, 'invalidateQueries'>,
  recordSnapshot: () => Promise<unknown>
): Promise<void> {
  try {
    await qc.invalidateQueries();
    try {
      await recordSnapshot();
    } catch (error) {
      console.warn('Recording the portfolio snapshot after a sync change failed', error);
    }
  } finally {
    await qc.invalidateQueries({ queryKey: ['portfolio-history'] }).catch(() => {});
  }
}
