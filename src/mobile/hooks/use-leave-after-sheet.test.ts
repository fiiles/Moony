import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { leaveAfterSheet } from './use-leave-after-sheet';

/** A window that holds listeners, so `popstate` can be dispatched without a browser. */
function stubWindow() {
  const target = new EventTarget();
  vi.stubGlobal('window', {
    addEventListener: target.addEventListener.bind(target),
    removeEventListener: target.removeEventListener.bind(target),
  });
  return { popstate: () => target.dispatchEvent(new Event('popstate')) };
}

describe('leaveAfterSheet', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('closes the sheet at once and leaves only when the sheet has popped its entry', () => {
    const win = stubWindow();
    const closeSheet = vi.fn();
    const leave = vi.fn();

    leaveAfterSheet(closeSheet, leave);
    expect(closeSheet).toHaveBeenCalledOnce();
    expect(leave).not.toHaveBeenCalled();

    win.popstate();
    expect(leave).toHaveBeenCalledOnce();
  });

  it('registers its listener before the sheet closes, so the sheet cannot pop unheard', () => {
    const win = stubWindow();
    const leave = vi.fn();
    // A sheet that pops its entry synchronously while closing
    leaveAfterSheet(() => win.popstate(), leave);
    expect(leave).toHaveBeenCalledOnce();
  });

  it('leaves once through the fallback when no popstate arrives', () => {
    stubWindow();
    const leave = vi.fn();

    leaveAfterSheet(vi.fn(), leave);
    vi.advanceTimersByTime(399);
    expect(leave).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(leave).toHaveBeenCalledOnce();
  });

  it('never leaves twice, whichever signal comes second', () => {
    const win = stubWindow();
    const leave = vi.fn();
    leaveAfterSheet(vi.fn(), leave);
    win.popstate();
    win.popstate();
    vi.advanceTimersByTime(1000);
    expect(leave).toHaveBeenCalledOnce();

    const late = vi.fn();
    leaveAfterSheet(vi.fn(), late);
    vi.advanceTimersByTime(400);
    win.popstate();
    expect(late).toHaveBeenCalledOnce();
  });
});
