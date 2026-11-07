import { describe, expect, it } from 'vitest';
import { createLockReturn } from './lock-return';

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
