import { describe, expect, it } from 'vitest';
import { createSheetHistory, type HistoryPort } from './sheet-history';

/**
 * A session history that behaves like the browser's: `go` is queued and takes effect later (then
 * the `popstate` arrives), a user's back takes effect at once.
 */
function setup() {
  const entries: unknown[] = [null];
  let index = 0;
  let queued: number | null = null;
  const port: HistoryPort = {
    get state() {
      return entries[index];
    },
    pushState(state) {
      entries.length = index + 1;
      entries.push(state);
      index++;
    },
    go(delta) {
      queued = delta;
    },
  };
  const sheets = createSheetHistory(port);
  return {
    sheets,
    /** Entries above the one the app started on. */
    depth: () => index,
    /** The browser applies a queued `go` and reports it. */
    settle() {
      if (queued === null) return;
      index += queued;
      queued = null;
      sheets.popstate();
    },
    /** The user presses Android back. */
    back() {
      index--;
      sheets.popstate();
    },
    /** The app pushes an entry of its own (a route change). */
    navigate() {
      port.pushState(null, '');
    },
  };
}

/** The microtask a closing sheet schedules has run. */
const tick = () => Promise.resolve();

describe('createSheetHistory', () => {
  it('gives each open sheet one entry', () => {
    const { sheets, depth } = setup();
    sheets.open(() => {});
    sheets.open(() => {});
    expect(depth()).toBe(2);
  });

  it('closes only the newest sheet on back and leaves the one beneath its entry', async () => {
    const { sheets, depth, back, settle } = setup();
    const closed: string[] = [];
    sheets.open(() => closed.push('below'));
    const releaseTop = sheets.open(() => closed.push('top'));
    back();
    expect(closed).toEqual(['top']);
    releaseTop(); // the sheet has closed itself in answer
    await tick();
    settle();
    expect(closed).toEqual(['top']);
    expect(depth()).toBe(1);
  });

  it('takes the entry off when the newest sheet closes by other means, without closing the one beneath', async () => {
    const { sheets, depth, settle } = setup();
    const closed: string[] = [];
    sheets.open(() => closed.push('below'));
    const releaseTop = sheets.open(() => closed.push('top'));
    releaseTop();
    await tick();
    settle(); // the popstate of our own step
    expect(closed).toEqual([]);
    expect(depth()).toBe(1);
  });

  it.each([
    ['newest first', [1, 0]],
    ['oldest first', [0, 1]],
  ])('leaves no entry behind when both sheets close together (%s)', async (_, order) => {
    const { sheets, depth, settle } = setup();
    const releases = [sheets.open(() => {}), sheets.open(() => {})];
    for (const i of order) releases[i]();
    await tick();
    settle();
    expect(depth()).toBe(0);
  });

  it('removes a closed sheet beneath once the sheet over it has gone', async () => {
    const { sheets, depth, back, settle } = setup();
    const releaseBelow = sheets.open(() => {});
    const releaseTop = sheets.open(() => {});
    releaseBelow(); // closes while the other is still over it
    await tick();
    expect(depth()).toBe(2);
    back();
    releaseTop();
    await tick();
    settle();
    expect(depth()).toBe(0);
  });

  it('does not touch the history when the app has moved on while the sheet was open', async () => {
    const { sheets, depth, navigate, settle } = setup();
    const release = sheets.open(() => {});
    navigate();
    release();
    await tick();
    settle(); // nothing was queued
    expect(depth()).toBe(2);
  });

  it('ignores back when no sheet is open', () => {
    const { back } = setup();
    expect(() => back()).not.toThrow();
  });
});
