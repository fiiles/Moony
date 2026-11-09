import { describe, expect, it } from 'vitest';
import { refreshAfterRemoteChanges } from './device-sync-refresh';

describe('refreshAfterRemoteChanges', () => {
  it('invalidates everything, records a snapshot, then refreshes portfolio history', async () => {
    const calls: string[] = [];
    const qc = {
      invalidateQueries: async (filters?: { queryKey?: readonly unknown[] }) => {
        calls.push(`invalidate:${filters?.queryKey ? JSON.stringify(filters.queryKey) : 'all'}`);
      },
    };
    const recordSnapshot = async () => {
      calls.push('snapshot');
    };

    await refreshAfterRemoteChanges(qc, recordSnapshot);

    expect(calls).toEqual(['invalidate:all', 'snapshot', 'invalidate:["portfolio-history"]']);
  });

  it('does not refresh portfolio history when the snapshot fails', async () => {
    const calls: string[] = [];
    const qc = {
      invalidateQueries: async () => {
        calls.push('invalidate');
      },
    };
    await expect(
      refreshAfterRemoteChanges(qc, async () => {
        throw new Error('boom');
      })
    ).rejects.toThrow('boom');
    expect(calls).toEqual(['invalidate']);
  });
});
