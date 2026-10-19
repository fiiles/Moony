import { describe, expect, it } from 'vitest';
import type { CategorizeQueue, CategorizeQueueRow, ChangedRow } from '@shared/schema';
import {
  SNOOZE_DAYS,
  doneCategories,
  effectiveOpenIds,
  nextSnapshot,
  openCount,
  queuePath,
  queueProgress,
  queueSummary,
  rowState,
  suggestionLabelKey,
  type LocalOutcome,
} from './categorize-queue';

const changed = (id: string): ChangedRow => ({
  id,
  previousCategoryId: null,
  previousSource: null,
});

/** A queue row with only what the library reads. */
const row = (id: string, accountId = 'acc-1'): CategorizeQueueRow =>
  ({ transaction: { id, bankAccountId: accountId }, suggestion: null }) as CategorizeQueueRow;

const queue = (ids: string[], total = ids.length, accountId = 'acc-1'): CategorizeQueue => ({
  rows: ids.map((id) => row(id, accountId)),
  total,
  snoozed: 0,
});

const categorized = (categoryId: string, batch: ChangedRow[] = []): LocalOutcome => ({
  kind: 'categorized',
  categoryId,
  batch,
});

describe('queueProgress', () => {
  it('counts the snapshot rows that have left the open set', () => {
    const open = new Set(['c', 'd']);
    expect(queueProgress(['a', 'b', 'c', 'd'], open)).toEqual({ handled: 2, total: 4 });
  });

  it('is nothing handled while every row is open, and all handled once none is', () => {
    expect(queueProgress(['a', 'b'], new Set(['a', 'b']))).toEqual({ handled: 0, total: 2 });
    expect(queueProgress(['a', 'b'], new Set())).toEqual({ handled: 2, total: 2 });
  });

  it('ignores open ids the snapshot does not hold (a payment that arrived later)', () => {
    expect(queueProgress(['a'], new Set(['a', 'z']))).toEqual({ handled: 0, total: 1 });
  });

  it('is empty for an empty snapshot', () => {
    expect(queueProgress([], new Set(['a']))).toEqual({ handled: 0, total: 0 });
  });
});

describe('rowState', () => {
  const none = new Map<string, LocalOutcome>();

  it('is open while the row is in the open set', () => {
    expect(rowState('a', new Set(['a']), none)).toBe('open');
  });

  it('is done once the row has left the open set without a local action', () => {
    // a payment another row's batch categorized, or one categorized elsewhere
    expect(rowState('a', new Set(['b']), none)).toBe('done');
  });

  it('prefers the local outcome over the open set, which may still be the answer before the refresh', () => {
    const local = new Map([['a', categorized('food')]]);
    expect(rowState('a', new Set(['a']), local)).toBe('done');
  });

  it('shows a snoozed row as left, whatever the open set says', () => {
    const local = new Map<string, LocalOutcome>([['a', { kind: 'left' }]]);
    expect(rowState('a', new Set(['a']), local)).toBe('left');
    expect(rowState('a', new Set(), local)).toBe('left');
  });

  it('keeps a batch-categorized row done without any local outcome of its own', () => {
    const local = new Map([['a', categorized('food', [changed('b'), changed('c')])]]);
    expect(rowState('b', new Set(['d']), local)).toBe('done');
  });
});

describe('suggestionLabelKey', () => {
  it('names each of the four sources and no two alike', () => {
    const keys = (['mcp', 'learned', 'rule', 'pack'] as const).map(suggestionLabelKey);
    expect(keys).toEqual([
      'queue.source.mcp',
      'queue.source.learned',
      'queue.source.rule',
      'queue.source.pack',
    ]);
    expect(new Set(keys).size).toBe(4);
  });
});

describe('doneCategories', () => {
  it('gives each categorized row its category, and the rows of its batch the same', () => {
    const local = new Map<string, LocalOutcome>([
      ['a', categorized('food', [changed('b'), changed('c')])],
      ['d', { kind: 'left' }],
    ]);
    expect(Object.fromEntries(doneCategories(local))).toEqual({
      a: 'food',
      b: 'food',
      c: 'food',
    });
  });

  it("lets a row's own pick win over a batch that reached it earlier", () => {
    const local = new Map<string, LocalOutcome>([
      ['a', categorized('food', [changed('b')])],
      ['b', categorized('fun')],
    ]);
    expect(doneCategories(local).get('b')).toBe('fun');
  });
});

describe('effectiveOpenIds', () => {
  it('is the answer minus what was handled here, so the progress does not wait for the refresh', () => {
    const local = new Map<string, LocalOutcome>([
      ['a', categorized('food')],
      ['b', { kind: 'left' }],
    ]);
    expect([...effectiveOpenIds(queue(['a', 'b', 'c']), local)]).toEqual(['c']);
  });

  it('is empty before the first answer', () => {
    expect(effectiveOpenIds(undefined, new Map()).size).toBe(0);
  });
});

describe('nextSnapshot', () => {
  const none = new Map<string, LocalOutcome>();

  it('waits for an answer that is not a stale one being refreshed', () => {
    expect(nextSnapshot(null, queue(['a']), false, none)).toBeNull();
    expect(nextSnapshot(null, undefined, true, none)).toBeNull();
  });

  it('takes the first settled answer', () => {
    const live = queue(['a', 'b']);
    expect(nextSnapshot(null, live, true, none)).toBe(live.rows);
  });

  it('keeps the rows while any of them is still open, whatever the answer lists', () => {
    const kept = queue(['a', 'b']).rows;
    expect(nextSnapshot(kept, queue(['b', 'z']), true, none)).toBe(kept);
  });

  it('takes the next rows once every row of the snapshot is dealt with and others wait', () => {
    // 600 payments, the first 500 were worked through: the answer lists the next ones
    const kept = queue(['a', 'b']).rows;
    const live = queue(['x', 'y'], 2);
    expect(nextSnapshot(kept, live, true, none)).toBe(live.rows);
  });

  it('does not fill again with rows this page has handled itself', () => {
    const kept = queue(['a']).rows;
    const local = new Map<string, LocalOutcome>([['a', categorized('food')]]);
    // a settled answer cannot list `a`, but a stale one could: nothing new is waiting
    expect(nextSnapshot(kept, queue(['a']), true, local)).toBe(kept);
  });

  it('keeps the rows of an emptied queue, which is then the "all done" state', () => {
    const kept = queue(['a']).rows;
    expect(nextSnapshot(kept, queue([]), true, none)).toBe(kept);
  });

  it('keeps the rows while the answer is being refreshed', () => {
    const kept = queue(['a']).rows;
    expect(nextSnapshot(kept, queue(['x']), false, none)).toBe(kept);
  });
});

describe('openCount', () => {
  it('counts the snapshot rows that are open', () => {
    const rows = queue(['a', 'b', 'c']).rows;
    expect(openCount(rows, new Set(['a', 'c']), queue(['a', 'c']))).toBe(2);
  });

  it('adds the payments beyond the cap of rows the answer lists', () => {
    const rows = queue(['a', 'b']).rows;
    // 502 waiting, 2 listed
    expect(openCount(rows, new Set(['a', 'b']), queue(['a', 'b'], 502))).toBe(502);
  });

  it('is the rows alone before the first answer', () => {
    expect(openCount([], new Set(), undefined)).toBe(0);
  });
});

describe('queueSummary', () => {
  it('is the total and the accounts holding the listed payments, each once', () => {
    const q: CategorizeQueue = {
      rows: [row('a', 'acc-1'), row('b', 'acc-2'), row('c', 'acc-1')],
      total: 7,
      snoozed: 3,
    };
    expect(queueSummary(q)).toEqual({ count: 7, accountIds: ['acc-1', 'acc-2'] });
  });

  it('is none before the answer', () => {
    expect(queueSummary(undefined)).toEqual({ count: 0, accountIds: [] });
  });
});

describe('queuePath', () => {
  it('names the account when exactly one holds the payments', () => {
    expect(queuePath(['acc-1'])).toBe('/finance/uncategorized?account=acc-1');
  });

  it('opens the queue of every account for none or several', () => {
    expect(queuePath([])).toBe('/finance/uncategorized');
    expect(queuePath(['a', 'b'])).toBe('/finance/uncategorized');
  });
});

describe('SNOOZE_DAYS', () => {
  it('is the 30 days of the spec', () => {
    expect(SNOOZE_DAYS).toBe(30);
  });
});
