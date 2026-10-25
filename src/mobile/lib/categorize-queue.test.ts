import { describe, expect, it } from 'vitest';
import type { CategorizeQueue, CategorizeQueueRow, ChangedRow } from '@shared/schema';
import {
  SNOOZE_DAYS,
  batchOffered,
  doneCategories,
  effectiveOpenIds,
  nextSnapshot,
  openCount,
  queuePath,
  queueProgress,
  queueSummary,
  releaseRow,
  rowState,
  suggestionLabelKey,
  withBatch,
  withPick,
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

  it('lets a batch that took a picked row along show its own category (it came later)', () => {
    // A batch only still holds a row picked here when the pick came first (`withPick` lets go of it)
    const local = new Map<string, LocalOutcome>([
      ['a', categorized('food', [changed('b')])],
      ['b', categorized('fun')],
    ]);
    expect(doneCategories(local).get('b')).toBe('food');
  });

  it('forgets the rows of a batch that was restored, and keeps the pick of its own row', () => {
    // the switch of "a" was turned off: "b" and "c" hold what they held before, the queue lists them again
    const local = new Map<string, LocalOutcome>([
      ['a', categorized('food', [changed('b'), changed('c')])],
      ['d', categorized('fun', [changed('e')])],
    ]);
    expect(Object.fromEntries(doneCategories(local, new Set(['a'])))).toEqual({
      a: 'food',
      d: 'fun',
      e: 'fun',
    });
  });

  it('still gives a restored row its own pick when it was categorized by hand afterwards', () => {
    const local = new Map<string, LocalOutcome>([
      ['a', categorized('food', [changed('b')])],
      ['b', categorized('fun')],
    ]);
    expect(doneCategories(local, new Set(['a'])).get('b')).toBe('fun');
  });
});

describe('the order of picks and batches', () => {
  const none = new Map<string, LocalOutcome>();
  const batchOf = (local: ReadonlyMap<string, LocalOutcome>, id: string) => {
    const outcome = local.get(id);
    return outcome?.kind === 'categorized' ? outcome.batch.map((row) => row.id) : [];
  };

  it('records a pick with nothing taken along yet, and a batch on the pick it came from', () => {
    const picked = withPick(none, 'a', 'food');
    expect(picked.get('a')).toEqual(categorized('food'));
    const taken = withBatch(picked, 'a', [changed('b'), changed('c')]);
    expect(batchOf(taken, 'a')).toEqual(['b', 'c']);
    expect(Object.fromEntries(doneCategories(taken))).toEqual({ a: 'food', b: 'food', c: 'food' });
  });

  it('ignores an empty batch and a batch whose pick is gone', () => {
    const picked = withPick(none, 'a', 'food');
    expect(withBatch(picked, 'a', [])).toBe(picked);
    expect(withBatch(none, 'a', [changed('b')])).toBe(none);
    const left = new Map<string, LocalOutcome>([['a', { kind: 'left' }]]);
    expect(withBatch(left, 'a', [changed('b')])).toBe(left);
  });

  it('lets an earlier batch go of a row picked again, so its undo cannot restore over the pick', () => {
    // "a" accepted, its learning took "b" and "c" along; then "b" is re-picked by hand
    let local = withBatch(withPick(none, 'a', 'food'), 'a', [changed('b'), changed('c')]);
    local = withPick(local, 'b', 'fun');
    expect(batchOf(local, 'a')).toEqual(['c']);
    expect(doneCategories(local).get('b')).toBe('fun');
    // the switch of "a" now gives back only "c"; "b" keeps the person's own pick
    expect(Object.fromEntries(doneCategories(local, new Set(['a'])))).toEqual({
      a: 'food',
      b: 'fun',
    });
  });

  it("shows a re-pick's batch on the row it took back along, and an earlier switch no longer reaches it", () => {
    // (a) of the review: the re-pick of "b" re-learns the payee and recategorizes "a" too
    let local = withBatch(withPick(none, 'a', 'food'), 'a', [changed('b')]);
    local = withPick(local, 'b', 'fun');
    local = withBatch(local, 'b', [{ id: 'a', previousCategoryId: 'food', previousSource: null }]);
    expect(Object.fromEntries(doneCategories(local))).toEqual({ a: 'fun', b: 'fun' });
    expect(batchOf(local, 'a')).toEqual([]);
    expect(batchOf(local, 'b')).toEqual(['a']);
    // nothing of "a" is offered back any more; turning off the switch of "b" gives "a" its pick back
    expect(batchOffered(local, new Set(['b']))).toBe(false);
    expect(doneCategories(local, new Set(['b'])).get('a')).toBe('food');
  });

  it("shows the batch's category on a row picked earlier that a later accept took along", () => {
    // (b) of the review: "a" picked as food, then the pill of "b" (same payee) accepted as fun
    let local: ReadonlyMap<string, LocalOutcome> = withPick(none, 'a', 'food');
    local = withPick(local, 'b', 'fun');
    local = withBatch(local, 'b', [{ id: 'a', previousCategoryId: 'food', previousSource: null }]);
    expect(doneCategories(local).get('a')).toBe('fun');
    // undone, "a" holds the pick it had before
    expect(doneCategories(local, new Set(['b'])).get('a')).toBe('food');
  });

  it('lets every earlier batch go of the rows a later batch took', () => {
    let local = withBatch(withPick(none, 'a', 'food'), 'a', [changed('x'), changed('y')]);
    local = withBatch(withPick(local, 'b', 'fun'), 'b', [changed('y'), changed('z')]);
    expect(batchOf(local, 'a')).toEqual(['x']);
    expect(batchOf(local, 'b')).toEqual(['y', 'z']);
    expect(doneCategories(local).get('y')).toBe('fun');
  });

  it('lets earlier batches go of a row once its own accept has landed, and leaves the rest alone', () => {
    const local = withBatch(withPick(none, 'a', 'food'), 'a', [changed('b'), changed('c')]);
    const landed = releaseRow(local, 'b');
    expect(batchOf(landed, 'a')).toEqual(['c']);
    expect(releaseRow(local, 'z')).toEqual(local);
    expect(local.get('a')).toEqual(categorized('food', [changed('b'), changed('c')]));
  });
});

describe('batchOffered', () => {
  it('is true while a pick that took other payments along can still be given back', () => {
    const local = new Map<string, LocalOutcome>([['a', categorized('food', [changed('b')])]]);
    expect(batchOffered(local, new Set())).toBe(true);
  });

  it('is false once its switch was turned off, for a pick that took nothing along and for a skipped row', () => {
    const local = new Map<string, LocalOutcome>([
      ['a', categorized('food', [changed('b')])],
      ['c', categorized('fun')],
      ['d', { kind: 'left' }],
    ]);
    expect(batchOffered(local, new Set(['a']))).toBe(false);
    expect(batchOffered(new Map(), new Set())).toBe(false);
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
  const none = new Map<string, LocalOutcome>();

  it('is the number of payments waiting', () => {
    expect(openCount(queue(['a', 'b', 'c']), none)).toBe(3);
  });

  it('counts the payments beyond the rows the answer lists', () => {
    // 502 waiting, the newest 2 listed
    expect(openCount(queue(['a', 'b'], 502), none)).toBe(502);
  });

  it('takes off what this page has handled while the answer still lists it', () => {
    const local = new Map<string, LocalOutcome>([
      ['a', categorized('food')],
      ['b', { kind: 'left' }],
    ]);
    expect(openCount(queue(['a', 'b', 'c']), local)).toBe(1);
  });

  it('does not take them off twice once the refreshed answer no longer lists them', () => {
    const local = new Map<string, LocalOutcome>([['a', categorized('food')]]);
    // 520 waiting; "a" handled; the answer after the refresh: 519 waiting and "a" gone from the rows
    expect(openCount(queue(['b', 'c'], 519), local)).toBe(519);
  });

  it('follows a window of 500 rows that moves on as payments are handled (more than 500 waiting)', () => {
    const ids = (from: number, n: number) => Array.from({ length: n }, (_, i) => `p${from + i}`);
    const handled = ids(0, 20);
    const local = new Map<string, LocalOutcome>(handled.map((id) => [id, categorized('food')]));
    // 520 waiting at the start, 20 handled: the refreshed answer says 500 and lists p20 to p519
    expect(openCount(queue(ids(20, 500), 500), local)).toBe(500);
    // the first accept, before its refresh: the answer is the old one
    expect(openCount(queue(ids(0, 500), 520), new Map([['p0', categorized('food')]]))).toBe(519);
    // after that refresh the window starts at p1 and reaches p500, a row the first answer never listed
    expect(openCount(queue(ids(1, 500), 519), new Map([['p0', categorized('food')]]))).toBe(519);
  });

  it('is nothing before the first answer', () => {
    expect(openCount(undefined, none)).toBe(0);
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
