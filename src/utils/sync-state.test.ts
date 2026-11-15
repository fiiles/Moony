import { describe, expect, it } from 'vitest';
import type { SyncPhase, SyncProblem, SyncStatus } from '@shared/schema';
import { isQuietlyOutOfReach, syncAge, syncState } from './sync-state';
import type { SyncAgeKey, SyncStateKind, SyncTone } from './sync-state';

const D = 86_400;
const now = 20_000 * D + 43_200;

function status(
  phase: SyncPhase,
  problem: SyncProblem | null = null,
  lastSuccessAt: number | null = null,
  enabledAt: number | null = null
): SyncStatus {
  return {
    phase,
    problem,
    lastSuccessAt,
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
    enabledAt,
  };
}

describe('syncState', () => {
  it('maps the spec rows', () => {
    const rows: [SyncStatus | undefined, SyncStateKind | null, SyncTone | null][] = [
      [undefined, null, null],
      [status('off'), null, null],
      [status('settingUp'), null, null],
      [status('idle', null, now - 240), 'synced', 'fresh'],
      [status('syncing'), 'syncing', 'neutral'],
      // Differs from Rust on purpose: the reminder stays `Some` for this input, the bar shows the round.
      [status('syncing', 'offline', now - 30 * D), 'syncing', 'neutral'],
      [status('syncing', null, now - 30 * D), 'syncing', 'neutral'],
      [status('idle', null, now - 30 * D), 'synced', 'fresh'],
      [status('waiting', null, now - 30 * D), 'waiting', 'stale'],
      [status('paused', 'offline', now - 6 * D), 'unreachable', 'neutral'],
      [status('paused', 'offline', now - 7 * D), 'unreachableLong', 'stale'],
      [status('error', 'relayError', now - D), 'unreachable', 'neutral'],
      [status('paused', 'rateLimited', now - D), 'unreachable', 'neutral'],
      [status('waiting', 'gap', now), 'waiting', 'stale'],
      [status('waiting', 'clockAhead', now), 'waiting', 'stale'],
      [status('paused', 'lapsed', now), 'paused', 'stale'],
      [status('paused', null, now), 'paused', 'stale'],
      [status('error', 'internal', now), 'error', 'stale'],
      [status('error', 'captureBroken', now), 'error', 'stale'],
    ];
    for (const [input, kind, tone] of rows) {
      const view = syncState(input, now);
      expect(view?.kind ?? null, JSON.stringify(input)).toBe(kind);
      expect(view?.tone ?? null).toBe(tone);
    }
  });

  it('switches to stale on the seventh UTC day', () => {
    const lateOnDay = 20_000 * D - 60; // 23:59 of the day before `now`'s day
    expect(syncState(status('paused', 'offline', lateOnDay - 5 * D), 20_000 * D + 60)?.kind).toBe(
      'unreachable'
    );
    expect(
      syncState(status('paused', 'offline', lateOnDay - 6 * D), 20_000 * D + 60)
    ).toMatchObject({
      kind: 'unreachableLong',
      days: 7,
      labelKey: 'indicator.unreachableLong',
      ageFrom: null,
    });
  });

  it('counts a device that never synced from when sync was turned on', () => {
    expect(syncState(status('paused', 'offline', null, now - 8 * D), now)).toMatchObject({
      kind: 'unreachableLong',
      days: 8,
    });
    expect(syncState(status('paused', 'offline', null, null), now)).toMatchObject({
      kind: 'unreachable',
      days: 0,
      ageFrom: null,
    });
  });

  it('never counts negative days for a success in the future', () => {
    expect(syncState(status('paused', 'offline', now + 3 * D), now)).toMatchObject({
      kind: 'unreachable',
      days: 0,
    });
  });

  it('keys the labels and the age by kind', () => {
    expect(syncState(status('idle', null, now - 240), now)).toMatchObject({
      labelKey: 'indicator.synced',
      rowKey: 'phone.row.synced',
      ageFrom: now - 240,
    });
  });
});

describe('syncAge', () => {
  it('gives the age in whole units', () => {
    const cases: [number, SyncAgeKey, number][] = [
      [0, 'justNow', 0],
      [59, 'justNow', 0],
      [60, 'minutes', 1],
      [3_599, 'minutes', 59],
      [3_600, 'hours', 1],
      [86_399, 'hours', 23],
      [86_400, 'days', 1],
      [-500, 'justNow', 0],
    ];
    for (const [elapsed, key, count] of cases) {
      expect(syncAge(now - elapsed, now)).toEqual({ key, count });
    }
  });
});

describe('isQuietlyOutOfReach', () => {
  const two = now - 2 * D;
  it('is quiet for a transient problem under 7 days, whatever the phase', () => {
    expect(isQuietlyOutOfReach(status('syncing', 'offline', two), now)).toBe(true);
    expect(isQuietlyOutOfReach(status('idle', 'offline', two), now)).toBe(true);
    expect(isQuietlyOutOfReach(status('paused', 'offline', two), now)).toBe(true);
    expect(isQuietlyOutOfReach(status('syncing', 'relayError', two), now)).toBe(true);
    expect(isQuietlyOutOfReach(status('idle', 'rateLimited', two), now)).toBe(true);
  });
  it('is not quiet after 7 days, even while a round runs', () => {
    expect(isQuietlyOutOfReach(status('syncing', 'offline', now - 8 * D), now)).toBe(false);
  });
  it('is not quiet for other problems or none', () => {
    expect(isQuietlyOutOfReach(status('paused', 'lapsed', two), now)).toBe(false);
    expect(isQuietlyOutOfReach(status('waiting', 'gap', now - 30 * D), now)).toBe(false);
    expect(isQuietlyOutOfReach(status('idle', null, two), now)).toBe(false);
  });
  it('counts from enabledAt without a success, and never a negative day count', () => {
    expect(isQuietlyOutOfReach(status('idle', 'offline', null, now - 9 * D), now)).toBe(false);
    expect(isQuietlyOutOfReach(status('idle', 'offline', null, null), now)).toBe(true);
    expect(isQuietlyOutOfReach(status('idle', 'offline', now + 5 * D), now)).toBe(true);
    expect(isQuietlyOutOfReach(status('idle', 'lapsed', now + 5 * D), now)).toBe(false);
  });
});
