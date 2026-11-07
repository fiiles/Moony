import { describe, expect, it } from 'vitest';
import type { NavEntry } from './nav-stack';
import { pathOnly, tabOfEntry, tabTapPlan, tabTrail } from './tab-memory';

const at = (path: string, scroll = 0): NavEntry => ({ path, scroll });
const stack = (...paths: string[]): NavEntry[] => paths.map((path) => at(path));

describe('pathOnly', () => {
  it('drops the search and the hash', () => {
    expect(pathOnly('/cashflow?o=-1')).toBe('/cashflow');
    expect(pathOnly('/finance#x')).toBe('/finance');
    expect(pathOnly('/finance')).toBe('/finance');
  });
});

describe('tabOfEntry', () => {
  it('reads the tab of a path with a search', () => {
    expect(tabOfEntry(at('/cashflow?view=plans'))).toBe('cashflow');
    expect(tabOfEntry(at('/settings'))).toBeNull();
    expect(tabOfEntry(undefined)).toBeNull();
  });
});

describe('tabTrail', () => {
  it('takes the screens of the tab on top, from its last root', () => {
    const s = stack('/', '/finance', '/finance/accounts/a', '/finance/loans/l');
    expect(tabTrail(s, 'finance')).toEqual({
      entries: stack('/finance', '/finance/accounts/a', '/finance/loans/l'),
      start: 1,
    });
  });

  it('starts at the last root when the tab was opened twice', () => {
    const s = stack('/finance', '/finance/accounts/a', '/finance', '/finance/loans/l');
    expect(tabTrail(s, 'finance').start).toBe(2);
  });

  it('keeps a trail entered from another tab without a root', () => {
    const s = stack('/', '/finance/accounts/a');
    expect(tabTrail(s, 'finance')).toEqual({ entries: stack('/finance/accounts/a'), start: 1 });
  });

  it('is empty when the top is on another tab', () => {
    expect(tabTrail(stack('/finance', '/cashflow'), 'finance')).toEqual({ entries: [], start: 2 });
  });

  it('keeps the search and the scroll of each screen', () => {
    const s = [at('/cashflow?o=-1', 300), at('/cashflow/budgets?o=-1', 40)];
    expect(tabTrail(s, 'cashflow').entries).toEqual(s);
  });
});

describe('tabTapPlan', () => {
  it('scrolls the root of the tab shown to its top', () => {
    expect(tabTapPlan(stack('/finance'), 'finance', undefined)).toEqual({ kind: 'scrollTop' });
    expect(tabTapPlan(stack('/cashflow?o=-2'), 'cashflow', undefined)).toEqual({
      kind: 'scrollTop',
    });
  });

  it('goes back to the root of the tab shown in one step', () => {
    const s = stack('/', '/finance', '/finance/accounts/a', '/finance/loans/l');
    expect(tabTapPlan(s, 'finance', undefined)).toEqual({ kind: 'popTo', index: 1 });
  });

  it('pushes the root when the tab shown was entered from another tab', () => {
    const s = stack('/', '/finance/accounts/a');
    expect(tabTapPlan(s, 'finance', undefined)).toEqual({
      kind: 'push',
      entries: [at('/finance')],
    });
  });

  it('pushes the root of another tab that remembers nothing', () => {
    expect(tabTapPlan(stack('/finance'), 'wealth', undefined)).toEqual({
      kind: 'push',
      entries: [at('/wealth')],
    });
    expect(tabTapPlan(stack('/finance'), 'wealth', [])).toEqual({
      kind: 'push',
      entries: [at('/wealth')],
    });
  });

  it('pushes the remembered trail of another tab with its scrolls', () => {
    const remembered = [at('/finance', 420), at('/finance/accounts/a', 80)];
    expect(tabTapPlan(stack('/cashflow'), 'finance', remembered)).toEqual({
      kind: 'push',
      entries: remembered,
    });
  });

  it('puts the root under a remembered trail that was entered from another tab', () => {
    const remembered = [at('/finance/accounts/a', 80)];
    expect(tabTapPlan(stack('/cashflow'), 'finance', remembered)).toEqual({
      kind: 'push',
      entries: [at('/finance'), at('/finance/accounts/a', 80)],
    });
  });

  it('pushes the root of Přehled from Settings, where no tab is shown', () => {
    expect(tabTapPlan(stack('/', '/settings'), 'overview', undefined)).toEqual({
      kind: 'push',
      entries: [at('/')],
    });
  });
});
