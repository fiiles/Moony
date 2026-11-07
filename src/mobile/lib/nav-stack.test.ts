import { describe, expect, it } from 'vitest';
import { navReduce, type NavEntry } from './nav-stack';

const stack = (...paths: string[]): NavEntry[] => paths.map((path) => ({ path, scroll: 0 }));

describe('navReduce', () => {
  it('adds a pushed screen on top, scrolled to its start', () => {
    const before: NavEntry[] = [{ path: '/finance', scroll: 420 }];
    expect(navReduce(before, { type: 'push', path: '/finance/accounts/a' })).toEqual([
      { path: '/finance', scroll: 420 },
      { path: '/finance/accounts/a', scroll: 0 },
    ]);
  });

  it('keeps the stack for a push of the top’s own path (a sheet’s history entry)', () => {
    const before = stack('/finance');
    expect(navReduce(before, { type: 'push', path: '/finance' })).toBe(before);
  });

  it('replaces the top path and keeps its scroll (a period change on the same screen)', () => {
    const before: NavEntry[] = [{ path: '/cashflow', scroll: 300 }];
    expect(navReduce(before, { type: 'replace', path: '/cashflow?o=-1' })).toEqual([
      { path: '/cashflow?o=-1', scroll: 300 },
    ]);
  });

  it('drops the top when back lands on the screen under it, with that screen’s scroll', () => {
    const before: NavEntry[] = [
      { path: '/finance', scroll: 420 },
      { path: '/finance/accounts/a', scroll: 80 },
    ];
    expect(navReduce(before, { type: 'pop', path: '/finance' })).toEqual([
      { path: '/finance', scroll: 420 },
    ]);
  });

  it('takes a pop to a screen it does not hold as a new one (forward, or history from before)', () => {
    expect(navReduce(stack('/finance'), { type: 'pop', path: '/wealth' })).toEqual(
      stack('/finance', '/wealth')
    );
  });

  it('leaves the stack alone for a pop that stays where it is (a sheet’s entry going)', () => {
    const before = stack('/finance', '/finance/accounts/a');
    expect(navReduce(before, { type: 'pop', path: '/finance/accounts/a' })).toBe(before);
  });

  it('drops every screen above a pop with a known target that lands where it was meant to', () => {
    const before: NavEntry[] = [
      { path: '/cashflow', scroll: 0 },
      { path: '/finance', scroll: 420 },
      { path: '/finance/accounts/a', scroll: 80 },
      { path: '/finance/accounts/a?tx=1', scroll: 0 },
    ];
    expect(navReduce(before, { type: 'pop', path: '/finance', to: 1 })).toEqual([
      { path: '/cashflow', scroll: 0 },
      { path: '/finance', scroll: 420 },
    ]);
  });

  it('reads a targeted pop that landed elsewhere as an ordinary pop', () => {
    const before = stack('/finance', '/finance/accounts/a', '/finance/loans/l');
    expect(navReduce(before, { type: 'pop', path: '/finance/accounts/a', to: 0 })).toEqual(
      stack('/finance', '/finance/accounts/a')
    );
  });
});
