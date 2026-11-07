import { describe, expect, it, vi } from 'vitest';
import { createTargetSlot, tapTarget } from './notification-target';

const paths = new Map([[7, '/finance/policies/p1']]);

describe('tapTarget', () => {
  it('reads the path from the extra (Android)', () => {
    expect(
      tapTarget(
        { actionId: 'tap', notification: { id: 1, extra: { path: '/settings/backup' } } },
        paths
      )
    ).toBe('/settings/backup');
  });

  it('falls back to the scheduled path of the id (iOS has no extra)', () => {
    expect(tapTarget({ actionId: 'tap', notification: { id: 7 } }, paths)).toBe(
      '/finance/policies/p1'
    );
  });

  it('ignores other actions, unknown ids and missing notifications', () => {
    expect(tapTarget({ actionId: 'dismiss', notification: { id: 7 } }, paths)).toBeNull();
    expect(tapTarget({ actionId: 'tap', notification: { id: 8 } }, paths)).toBeNull();
    expect(tapTarget({ actionId: 'tap', notification: null }, paths)).toBeNull();
    expect(tapTarget({ actionId: 'tap' }, paths)).toBeNull();
  });

  it('never accepts a path outside the shell', () => {
    expect(
      tapTarget({ actionId: 'tap', notification: { id: 1, extra: { path: '/auth' } } }, paths)
    ).toBeNull();
    expect(
      tapTarget(
        { actionId: 'tap', notification: { id: 7, extra: { path: 'https://x.example' } } },
        paths
      )
    ).toBe('/finance/policies/p1');
  });
});

describe('createTargetSlot', () => {
  it('holds the newest target until it is taken, once', () => {
    const slot = createTargetSlot();
    slot.offer('/');
    slot.offer('/settings/backup');
    expect(slot.peek()).toBe('/settings/backup');
    expect(slot.take()).toBe('/settings/backup');
    expect(slot.take()).toBeNull();
  });

  it('refuses a path outside the shell', () => {
    const slot = createTargetSlot();
    slot.offer('/auth');
    expect(slot.peek()).toBeNull();
  });

  it('tells its listeners about an offer and a take', () => {
    const slot = createTargetSlot();
    const listener = vi.fn();
    const stop = slot.subscribe(listener);
    slot.offer('/');
    slot.take();
    slot.take();
    expect(listener).toHaveBeenCalledTimes(2);
    stop();
    slot.offer('/');
    expect(listener).toHaveBeenCalledTimes(2);
  });
});
