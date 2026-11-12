import { describe, expect, it } from 'vitest';
import type { HeldGapView, SyncProblem } from '@shared/schema';
import {
  aheadGap,
  aheadText,
  firstSkippableAt,
  heldFor,
  skipGapTexts,
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
  clockAhead: ['none', 'none'],
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

  it('offers to skip a held change dated more than ten years ahead once it may be', () => {
    expect(problemAction('clockAhead', false, null)).toBe('none');
    expect(problemAction('clockAhead', true, gap({ kind: 'ahead' }))).toBe('skipMissing');
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
      if (problem === 'gap' || problem === 'clockAhead') continue;
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
    missingChanges: 1,
    since,
    skippableAt: since + 14 * DAY,
    datedAt: null,
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

  it('offers a missing change before one waiting for a parent', () => {
    const parent = gap({ deviceId: 'aa'.repeat(16), kind: 'parent', since: SINCE - 5 * DAY });
    const missing = gap({ deviceId: 'bb'.repeat(16), since: SINCE });
    expect(skippableGap([parent, missing], SINCE + 15 * DAY)).toBe(missing);
    expect(skippableGap([parent], SINCE + 15 * DAY)).toBe(parent);
  });
});

describe('firstSkippableAt', () => {
  it('is the earliest skippable time while none may be skipped yet', () => {
    const later = gap({ deviceId: 'aa'.repeat(16), since: SINCE + 3 * DAY });
    expect(firstSkippableAt(undefined, SINCE)).toBeNull();
    expect(firstSkippableAt([later, gap()], SINCE + DAY)).toBe(SINCE + 14 * DAY);
    expect(firstSkippableAt([later, gap()], SINCE + 14 * DAY)).toBeNull();
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

describe('heldFor', () => {
  it('gives a gap its missing and parent changes, clockAhead its changes dated ahead', () => {
    const missing = gap({ deviceId: 'aa'.repeat(16) });
    const parent = gap({ deviceId: 'bb'.repeat(16), kind: 'parent' });
    const ahead = gap({ deviceId: 'cc'.repeat(16), kind: 'ahead' });
    const all = [missing, parent, ahead];
    expect(heldFor('gap', all)).toEqual([missing, parent]);
    expect(heldFor('clockAhead', all)).toEqual([ahead]);
    expect(heldFor('quarantined', all)).toEqual([]);
    expect(heldFor('gap', undefined)).toEqual([]);
  });
});

describe('aheadGap and aheadText', () => {
  const t = (key: string, options?: Record<string, unknown>) =>
    `${key} ${JSON.stringify(options ?? {})}`;
  const dateText = (seconds: number) => `day ${seconds}`;

  it('is the oldest change dated ahead, named with its device and date', () => {
    const older = gap({ kind: 'ahead', since: SINCE - DAY, datedAt: 4_000_000_000 });
    const younger = gap({ deviceId: 'dd'.repeat(16), kind: 'ahead', since: SINCE });
    expect(aheadGap([gap(), younger, older])).toBe(older);
    expect(aheadGap([gap()])).toBeNull();
    expect(aheadText(older, t, dateText)).toBe(
      'alert.clockAhead.text {"device":"Work laptop","date":"day 4000000000"}'
    );
    expect(aheadText(younger, t, dateText)).toBe(
      'alert.clockAhead.textFar {"device":"Work laptop"}'
    );
  });

  it('gives the skip confirmation of a change dated ahead its own texts', () => {
    const texts = skipGapTexts(gap({ kind: 'ahead', datedAt: 4_000_000_000 }), t, dateText);
    expect(texts.title).toBe('skipGap.titleAhead {}');
    expect(texts.description).toBe(
      'skipGap.descriptionAhead {"device":"Work laptop","date":"day 4000000000"}'
    );
    expect(texts.skipped).toBe('skipGap.successAhead {}');
    expect(texts.unchanged).toBe('skipGap.appliedAhead {}');
    expect(skipGapTexts(gap({ kind: 'ahead' }), t, dateText).description).toBe(
      'skipGap.descriptionAheadFar {"device":"Work laptop"}'
    );
    expect(skipGapTexts(gap(), t, dateText).title).toBe('skipGap.title {"count":1}');
    expect(skipGapTexts(gap({ kind: 'parent' }), t, dateText).description).toBe(
      'skipGap.descriptionParent {"device":"Work laptop"}'
    );
  });
});
