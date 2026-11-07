import { describe, expect, it } from 'vitest';
import {
  PULL_HOLD_PX,
  PULL_IDLE,
  PULL_MAX_PX,
  PULL_TRIGGER_PX,
  holdsUndecidedMove,
  pullDirection,
  pullDistance,
  pullOffset,
  pullProgress,
  pullReducer,
  type PullEvent,
  type PullState,
} from './pull-refresh';

const run = (events: PullEvent[], from: PullState = PULL_IDLE) => events.reduce(pullReducer, from);
const start = (atTop = true): PullEvent => ({ type: 'start', x: 100, y: 200, atTop });
const move = (dy: number, dx = 0): PullEvent => ({ type: 'move', x: 100 + dx, y: 200 + dy });

describe('pullDistance', () => {
  it('is damped: never more than the finger and never past the maximum', () => {
    expect(pullDistance(0)).toBe(0);
    expect(pullDistance(-40)).toBe(0);
    for (const dy of [10, 50, 100, 200, 400]) expect(pullDistance(dy)).toBeLessThan(dy);
    expect(pullDistance(10_000)).toBeLessThanOrEqual(PULL_MAX_PX);
    expect(pullDistance(120)).toBeGreaterThan(pullDistance(100));
  });

  it('reaches the trigger after a deliberate pull, not a nudge', () => {
    expect(pullDistance(40)).toBeLessThan(PULL_TRIGGER_PX);
    expect(pullDistance(160)).toBeGreaterThanOrEqual(PULL_TRIGGER_PX);
  });
});

describe('pullDirection', () => {
  it('waits for the slop, then pulls only downwards and mostly vertical', () => {
    expect(pullDirection(2, 3)).toBeNull();
    expect(pullDirection(0, 20)).toBe('pull');
    expect(pullDirection(0, -20)).toBe('none');
    expect(pullDirection(20, 15)).toBe('none');
  });
});

describe('pullReducer', () => {
  it('pulls, arms past the trigger and refreshes on release', () => {
    const pulled = run([start(), move(20), move(200)]);
    expect(pulled).toMatchObject({ kind: 'pulling', armed: true });
    const refreshing = pullReducer(pulled, { type: 'end' });
    expect(refreshing).toEqual({ kind: 'refreshing' });
    expect(pullOffset(refreshing)).toBe(PULL_HOLD_PX);
    expect(pullReducer(refreshing, { type: 'done' })).toEqual(PULL_IDLE);
  });

  it('settles back on a release short of the trigger', () => {
    const pulled = run([start(), move(20), move(60)]);
    expect(pulled).toMatchObject({ kind: 'pulling', armed: false });
    expect(pullReducer(pulled, { type: 'end' })).toEqual(PULL_IDLE);
  });

  it('disarms when the finger goes back up before release', () => {
    const back = run([start(), move(20), move(200), move(30)]);
    expect(back).toMatchObject({ kind: 'pulling', armed: false });
  });

  it('lets go once the finger is back above its start, so the drag can scroll', () => {
    expect(run([start(), move(20), move(-50)])).toEqual({ kind: 'ignored' });
    expect(run([start(), move(20), move(0), move(200)])).toEqual({ kind: 'ignored' });
    expect(run([start(), move(20), move(-50), { type: 'end' }])).toEqual(PULL_IDLE);
  });

  it('ignores a screen scrolled down, a scroll up and a sideways drag', () => {
    expect(run([start(false), move(200)])).toEqual({ kind: 'ignored' });
    expect(run([start(), move(-20)])).toEqual({ kind: 'ignored' });
    expect(run([start(), move(10, 30)])).toEqual({ kind: 'ignored' });
    expect(run([start(), move(10, 30), move(300)])).toEqual({ kind: 'ignored' });
    expect(run([start(false), move(200), { type: 'end' }])).toEqual(PULL_IDLE);
  });

  it('keeps refreshing through new touches until done', () => {
    const refreshing: PullState = { kind: 'refreshing' };
    expect(run([start(), move(200), { type: 'end' }], refreshing)).toEqual(refreshing);
  });
});

describe('holdsUndecidedMove', () => {
  it('holds a downward move back while the drag is undecided, and nothing else', () => {
    const tracking = run([start()]);
    expect(holdsUndecidedMove(tracking, 100, 203)).toBe(true);
    expect(holdsUndecidedMove(tracking, 100, 197)).toBe(false);
    expect(holdsUndecidedMove(PULL_IDLE, 100, 250)).toBe(false);
    expect(holdsUndecidedMove(run([start(), move(20)]), 100, 250)).toBe(false);
  });

  it('leaves a mostly sideways start to the WebView (a horizontal scroller)', () => {
    const tracking = run([start()]);
    expect(holdsUndecidedMove(tracking, 106, 203)).toBe(false);
    expect(holdsUndecidedMove(tracking, 102, 205)).toBe(true);
  });
});

describe('the indicator', () => {
  it('fills towards the trigger and is full while refreshing', () => {
    expect(pullProgress(PULL_IDLE)).toBe(0);
    expect(pullProgress(run([start(), move(20), move(60)]))).toBeGreaterThan(0);
    expect(pullProgress(run([start(), move(20), move(400)]))).toBe(1);
    expect(pullProgress({ kind: 'refreshing' })).toBe(1);
    expect(pullOffset(PULL_IDLE)).toBe(0);
  });
});
