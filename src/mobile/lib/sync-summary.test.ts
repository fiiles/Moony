import { describe, expect, it } from 'vitest';
import type { SyncPhase, SyncProblem, SyncStatus } from '@shared/schema';
import { syncRowSummary } from './sync-summary';

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
  };
}

describe('syncRowSummary', () => {
  it('maps every phase and the offline problem to a row summary', () => {
    const table: Array<[SyncStatus | undefined, string, string]> = [
      [undefined, 'phone.row.off', 'neutral'],
      [status('off'), 'phone.row.off', 'neutral'],
      [status('settingUp'), 'phone.row.off', 'neutral'],
      [status('idle'), 'phone.row.synced', 'fresh'],
      [status('syncing'), 'phone.row.syncing', 'neutral'],
      [status('paused', 'offline'), 'phone.row.offline', 'stale'],
      [status('paused', 'lapsed'), 'phone.row.paused', 'stale'],
      [status('paused'), 'phone.row.paused', 'stale'],
      [status('error', 'internal'), 'phone.row.error', 'stale'],
      [status('error'), 'phone.row.error', 'stale'],
      [status('waiting', 'gap'), 'phone.row.waiting', 'stale'],
    ];
    for (const [input, key, tone] of table) {
      expect(syncRowSummary(input)).toEqual({ key, tone });
    }
  });
});
