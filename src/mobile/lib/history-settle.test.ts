import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { afterHistorySettles } from './history-settle';

/** An event target that holds its listeners and can raise a `popstate`. */
function fakeTarget() {
  const listeners = new Set<EventListenerOrEventListenerObject>();
  const target: Pick<EventTarget, 'addEventListener' | 'removeEventListener'> = {
    addEventListener: (_type, listener) => {
      if (listener) listeners.add(listener);
    },
    removeEventListener: (_type, listener) => {
      if (listener) listeners.delete(listener);
    },
  };
  const popstate = () =>
    [...listeners].forEach((listener) =>
      typeof listener === 'function' ? listener(new Event('popstate')) : undefined
    );
  return { target, listeners, popstate };
}

describe('afterHistorySettles', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('runs once on the first popstate and clears the timer and the listener', () => {
    const { target, listeners, popstate } = fakeTarget();
    const run = vi.fn();
    afterHistorySettles(target, run);
    expect(listeners.size).toBe(1);

    popstate();
    expect(run).toHaveBeenCalledTimes(1);
    expect(listeners.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);

    vi.advanceTimersByTime(1000);
    expect(run).toHaveBeenCalledTimes(1);
  });

  it('runs once on the timer when no popstate comes, and lets go of the listener', () => {
    const { target, listeners, popstate } = fakeTarget();
    const run = vi.fn();
    afterHistorySettles(target, run, 350);

    vi.advanceTimersByTime(349);
    expect(run).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(run).toHaveBeenCalledTimes(1);
    expect(listeners.size).toBe(0);

    popstate();
    expect(run).toHaveBeenCalledTimes(1);
  });

  it('waits 350 ms by default', () => {
    const { target } = fakeTarget();
    const run = vi.fn();
    afterHistorySettles(target, run);
    vi.advanceTimersByTime(349);
    expect(run).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(run).toHaveBeenCalledTimes(1);
  });

  it('does nothing once cancelled, by popstate or by timer', () => {
    const { target, listeners, popstate } = fakeTarget();
    const run = vi.fn();
    const cancel = afterHistorySettles(target, run);
    cancel();
    expect(listeners.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);

    popstate();
    vi.advanceTimersByTime(1000);
    expect(run).not.toHaveBeenCalled();
  });

  it('ignores a second popstate after it ran', () => {
    const { target, popstate } = fakeTarget();
    const run = vi.fn();
    afterHistorySettles(target, run);
    popstate();
    popstate();
    expect(run).toHaveBeenCalledTimes(1);
  });
});
