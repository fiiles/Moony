import { describe, expect, it } from 'vitest';
import type { SyncPhase, SyncProblem, SyncStatus } from '@shared/schema';
import { syncIndicator } from './device-sync-indicator';

function status(phase: SyncPhase, problem: SyncProblem | null = null): SyncStatus {
  return {
    phase,
    problem,
    lastSuccessAt: null,
    pendingChanges: 0,
    heldDevices: 0,
    refusedRows: 0,
    paidUntil: null,
    deletesAt: null,
    clockAheadUntil: null,
    devTools: false,
    ownServer: false,
    serverAddress: null,
    serverInsecure: false,
    enabledAt: null,
  };
}

describe('syncIndicator', () => {
  it('hides the indicator when sync is off or being set up', () => {
    expect(syncIndicator(undefined)).toBeNull();
    expect(syncIndicator(status('off'))).toBeNull();
    expect(syncIndicator(status('settingUp'))).toBeNull();
    expect(syncIndicator(status('off', 'offline'))).toBeNull();
  });

  it('maps every phase and the offline problem to a tone and label', () => {
    const table: [SyncPhase, SyncProblem | null, unknown][] = [
      ['idle', null, { tone: 'fresh', labelKey: 'indicator.synced' }],
      ['syncing', null, { tone: 'neutral', labelKey: 'indicator.syncing' }],
      ['paused', null, { tone: 'stale', labelKey: 'indicator.paused' }],
      ['error', null, { tone: 'stale', labelKey: 'indicator.error' }],
      ['waiting', 'gap', { tone: 'stale', labelKey: 'indicator.waiting' }],
      ['waiting', 'offline', { tone: 'stale', labelKey: 'indicator.offline' }],
      ['paused', 'lapsed', { tone: 'stale', labelKey: 'indicator.paused' }],
      ['error', 'internal', { tone: 'stale', labelKey: 'indicator.error' }],
      ['idle', 'offline', { tone: 'stale', labelKey: 'indicator.offline' }],
      ['syncing', 'offline', { tone: 'stale', labelKey: 'indicator.offline' }],
      ['paused', 'offline', { tone: 'stale', labelKey: 'indicator.offline' }],
      ['error', 'offline', { tone: 'stale', labelKey: 'indicator.offline' }],
    ];
    for (const [phase, problem, expected] of table) {
      expect(syncIndicator(status(phase, problem)), `${phase}/${problem}`).toEqual(expected);
    }
  });
});
