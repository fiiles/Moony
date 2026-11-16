import { describe, expect, it } from 'vitest';
import { SYNCING_REFETCH_MS, syncStatusRefetchInterval } from './device-sync-refetch';

describe('syncStatusRefetchInterval', () => {
  it('polls while the cached phase is syncing', () => {
    expect(syncStatusRefetchInterval({ phase: 'syncing' })).toBe(SYNCING_REFETCH_MS);
  });

  it.each(['idle', 'paused', 'error', 'off', 'waiting', 'settingUp'])('rests in %s', (phase) => {
    expect(syncStatusRefetchInterval({ phase })).toBe(false);
  });

  it('rests while nothing is cached', () => {
    expect(syncStatusRefetchInterval(undefined)).toBe(false);
  });
});
