import { describe, expect, it, vi } from 'vitest';
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

  it('still refreshes portfolio history and resolves when the snapshot fails', async () => {
    const calls: string[] = [];
    const qc = {
      invalidateQueries: async (filters?: { queryKey?: readonly unknown[] }) => {
        calls.push(`invalidate:${filters?.queryKey ? JSON.stringify(filters.queryKey) : 'all'}`);
      },
    };
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    await expect(
      refreshAfterRemoteChanges(qc, async () => {
        calls.push('snapshot');
        throw new Error('boom');
      })
    ).resolves.toBeUndefined();
    expect(calls).toEqual(['invalidate:all', 'snapshot', 'invalidate:["portfolio-history"]']);
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });
});
