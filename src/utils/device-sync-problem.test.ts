import { describe, expect, it } from 'vitest';
import type { HeldGapView, SyncProblem } from '@shared/schema';
import {
  gapSkippable,
  heldDays,
  heldGapDevice,
  problemAction,
  skippableGap,
  type SyncProblemAction,
} from './device-sync-problem';

const TABLE: Record<SyncProblem, [SyncProblemAction, SyncProblemAction]> = {
  // [normal server, self-hosted]
  captureBroken: ['repair', 'repair'],
  vaultGone: ['turnOffOrRetry', 'turnOffOrRetry'],
  lapsed: ['renew', 'askOperator'],
  quotaExceeded: ['none', 'none'],
  snapshotTooLarge: ['none', 'none'],
  newerSchema: ['updateApp', 'updateApp'],
  gap: ['none', 'none'],
  undecryptable: ['none', 'none'],
  invalidChain: ['none', 'none'],
  missingParent: ['none', 'none'],
  quarantined: ['none', 'none'],
  offline: ['none', 'none'],
  relayError: ['none', 'none'],
  rateLimited: ['none', 'none'],
  unauthorized: ['none', 'none'],
  keyRotated: ['none', 'none'],
  secureStorage: ['none', 'none'],
  serverWentBack: ['none', 'none'],
  internal: ['none', 'none'],
};

describe('problemAction', () => {
  it.each(Object.entries(TABLE) as [SyncProblem, [SyncProblemAction, SyncProblemAction]][])(
    '%s',
    (problem, [normal, selfHosted]) => {
      expect(problemAction(problem, false)).toBe(normal);
      expect(problemAction(problem, true)).toBe(selfHosted);
    }
  );

  it('offers no destructive action for a gap that may not be skipped yet', () => {
    expect(problemAction('gap', false, null)).toBe('none');
    expect(problemAction('gap', true, null)).toBe('none');
  });

  it('offers to skip a skippable held gap, never a re-sync', () => {
    expect(problemAction('gap', false, gap())).toBe('skipMissing');
    expect(problemAction('gap', true, gap())).toBe('skipMissing');
  });

  it('leaves the other problems as they are when a held gap is skippable', () => {
    for (const [problem, [normal, selfHosted]] of Object.entries(TABLE) as [
      SyncProblem,
      [SyncProblemAction, SyncProblemAction],
    ][]) {
      if (problem === 'gap') continue;
      expect(problemAction(problem, false, gap())).toBe(normal);
      expect(problemAction(problem, true, gap())).toBe(selfHosted);
    }
  });
});

const DAY = 86_400;
const SINCE = 1_772_323_200;

function gap(overrides: Partial<HeldGapView> = {}): HeldGapView {
  const since = overrides.since ?? SINCE;
  return {
    deviceId: '02'.repeat(16),
    deviceName: 'Work laptop',
    kind: 'missing',
    since,
    skippableAt: since + 14 * DAY,
    ...overrides,
  };
}

describe('gapSkippable', () => {
  it('is false before 14 days and true from then on', () => {
    expect(gapSkippable(gap(), SINCE + 13 * DAY)).toBe(false);
    expect(gapSkippable(gap(), SINCE + 14 * DAY - 1)).toBe(false);
    expect(gapSkippable(gap(), SINCE + 14 * DAY)).toBe(true);
    expect(gapSkippable(gap(), SINCE + 30 * DAY)).toBe(true);
  });
});

describe('skippableGap', () => {
  it('is the oldest skippable held gap, or null', () => {
    const older = gap({ deviceId: 'aa'.repeat(16), since: SINCE - 5 * DAY });
    const younger = gap({ deviceId: 'bb'.repeat(16), since: SINCE + 10 * DAY });
    expect(skippableGap(undefined, SINCE)).toBeNull();
    expect(skippableGap([younger], SINCE + 15 * DAY)).toBeNull();
    expect(skippableGap([younger, older], SINCE + 15 * DAY)).toBe(older);
  });
});

describe('heldDays', () => {
  it('counts whole days', () => {
    expect(heldDays(gap(), SINCE + 14 * DAY + 3600)).toBe(14);
    expect(heldDays(gap(), SINCE - 10)).toBe(0);
  });
});

describe('heldGapDevice', () => {
  it('is the device name, else the start of its ID', () => {
    expect(heldGapDevice(gap())).toBe('Work laptop');
    expect(heldGapDevice(gap({ deviceName: null }))).toBe('02020202');
  });
});
