import { describe, expect, it, vi } from 'vitest';
import { createLockReturn, createReturnWalk, isLateLanding } from './lock-return';

describe('createLockReturn', () => {
  it('remembers the screen and how far back it is: the lock entry and the open sheets', () => {
    const store = createLockReturn();
    store.locked('/finance/accounts/a', 2);
    expect(store.take()).toEqual({ path: '/finance/accounts/a', depth: 3 });
  });

  it('counts only the lock entry without a sheet open', () => {
    const store = createLockReturn();
    store.locked('/cashflow?o=-1', 0);
    expect(store.take()).toEqual({ path: '/cashflow?o=-1', depth: 1 });
  });

  it('is read once', () => {
    const store = createLockReturn();
    store.locked('/wealth', 0);
    store.take();
    expect(store.take()).toBeNull();
  });

  it('keeps nothing for a lock on a page outside the shell', () => {
    const store = createLockReturn();
    store.locked('/wealth', 0);
    store.locked('/auth', 0);
    expect(store.take()).toBeNull();
  });

  it('forgets on clear', () => {
    const store = createLockReturn();
    store.locked('/wealth', 0);
    store.clear();
    expect(store.take()).toBeNull();
  });
});

describe('createReturnWalk', () => {
  it('runs at once what asks while no walk is under way', () => {
    const walk = createReturnWalk();
    const run = vi.fn();
    walk.whenDone(run);
    expect(run).toHaveBeenCalledTimes(1);
  });

  it('holds what asks during the walk until it ends, in order, then tells the listeners', () => {
    const walk = createReturnWalk();
    const order: string[] = [];
    walk.subscribe(() => order.push(walk.active() ? 'began' : 'ended'));
    walk.begin();
    walk.whenDone(() => order.push('a'));
    walk.whenDone(() => order.push('b'));
    expect(order).toEqual(['began']);
    walk.end();
    expect(order).toEqual(['began', 'a', 'b', 'ended']);
    walk.end();
    expect(order).toHaveLength(4);
  });
});

describe('isLateLanding', () => {
  it('is the walk landing on the screen it went to, on that screen’s own entry', () => {
    expect(isLateLanding('/finance/accounts/a', null, '/finance/accounts/a')).toBe(true);
    expect(isLateLanding('/cashflow?o=-1', { other: 1 }, '/cashflow?o=-1')).toBe(true);
  });

  it('is not a back to another screen, nor to an entry a sheet pushed', () => {
    expect(isLateLanding('/finance', null, '/finance/accounts/a')).toBe(false);
    expect(isLateLanding('/cashflow', null, '/cashflow?o=-1')).toBe(false);
    expect(isLateLanding('/finance/accounts/a', { moonySheet: '3' }, '/finance/accounts/a')).toBe(
      false
    );
  });
});
