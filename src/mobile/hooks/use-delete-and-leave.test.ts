import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { leaveDeleted } from './use-delete-and-leave';

const pathOf = (id: string) => `/finance/policies/${id}`;
const keysOf = (id: string) => [
  ['insurance-documents', id],
  ['insurance-real-estate', id],
];

/** The page the window is on; `go` is the person (or `history.back()`) moving on. */
function stubWindow(pathname: string) {
  const location = { pathname };
  vi.stubGlobal('window', { location });
  return {
    go: (to: string) => {
      location.pathname = to;
    },
  };
}

function leave(id = 'p1') {
  const deps = { back: vi.fn(), replace: vi.fn(), removeQueries: vi.fn() };
  leaveDeleted({ home: '/finance', pathOf, keysOf }, deps, id);
  return deps;
}

describe('leaveDeleted', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('goes back while the screen is still the deleted record', () => {
    stubWindow(pathOf('p1'));
    const deps = leave();
    expect(deps.back).toHaveBeenCalledOnce();
  });

  it('does not go back when the person has already left during a slow delete', () => {
    stubWindow('/finance');
    const deps = leave();
    expect(deps.back).not.toHaveBeenCalled();
    // The record's own queries still go
    vi.advanceTimersByTime(250);
    expect(deps.removeQueries).toHaveBeenCalledTimes(2);
    expect(deps.replace).not.toHaveBeenCalled();
  });

  it('removes the record’s own queries only after the guard, so a re-render cannot rebuild them empty', () => {
    const win = stubWindow(pathOf('p1'));
    const deps = leave();
    vi.advanceTimersByTime(249);
    expect(deps.removeQueries).not.toHaveBeenCalled();

    // history.back() has landed by then
    win.go('/finance');
    vi.advanceTimersByTime(1);
    expect(deps.removeQueries).toHaveBeenCalledWith(['insurance-documents', 'p1']);
    expect(deps.removeQueries).toHaveBeenCalledWith(['insurance-real-estate', 'p1']);
    expect(deps.replace).not.toHaveBeenCalled();
  });

  it('replaces a restored deep link, which had nowhere to go back to, with the tab root', () => {
    stubWindow(pathOf('p1'));
    const deps = leave();
    vi.advanceTimersByTime(250);
    expect(deps.replace).toHaveBeenCalledWith('/finance');
    expect(deps.removeQueries).toHaveBeenCalledTimes(2);
  });

  it('never sends a person who is on another record anywhere', () => {
    stubWindow(pathOf('p2'));
    const deps = leave('p1');
    vi.advanceTimersByTime(250);
    expect(deps.back).not.toHaveBeenCalled();
    expect(deps.replace).not.toHaveBeenCalled();
    expect(deps.removeQueries).toHaveBeenCalledWith(['insurance-documents', 'p1']);
  });
});
