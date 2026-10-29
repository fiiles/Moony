import { describe, expect, it } from 'vitest';
import {
  idsInScope,
  pageCheckState,
  selectedRows,
  withPageSelected,
  withToggled,
} from './ledger-selection';

describe('idsInScope', () => {
  it('keeps the selection while the scope is the same', () => {
    const ids = new Set(['a', 'b']);
    expect(idsInScope({ scope: 'p1', ids }, 'p1')).toBe(ids);
  });

  it('reads as empty once the page, filter or sort (the scope) changed', () => {
    expect(idsInScope({ scope: 'p1', ids: new Set(['a']) }, 'p2').size).toBe(0);
  });
});

describe('withToggled', () => {
  it('adds and removes one id without touching the original', () => {
    const ids = new Set(['a']);
    expect([...withToggled(ids, 'b', true)]).toEqual(['a', 'b']);
    expect([...withToggled(ids, 'a', false)]).toEqual([]);
    expect([...ids]).toEqual(['a']);
  });
});

describe('pageCheckState', () => {
  it('is unchecked with nothing selected or an empty page', () => {
    expect(pageCheckState(['a', 'b'], new Set())).toBe(false);
    expect(pageCheckState([], new Set(['a']))).toBe(false);
  });

  it('is indeterminate when only part of the page is selected', () => {
    expect(pageCheckState(['a', 'b'], new Set(['b']))).toBe('indeterminate');
  });

  it('is checked when every row of the page is selected', () => {
    expect(pageCheckState(['a', 'b'], new Set(['a', 'b']))).toBe(true);
  });

  it('ignores ids that are not on the page', () => {
    expect(pageCheckState(['a'], new Set(['gone']))).toBe(false);
    expect(pageCheckState(['a'], new Set(['a', 'gone']))).toBe(true);
  });
});

describe('withPageSelected', () => {
  it('selects every row of the page', () => {
    expect([...withPageSelected(new Set(['a']), ['a', 'b', 'c'], true)]).toEqual(['a', 'b', 'c']);
  });

  it('clears the rows of the page', () => {
    expect([...withPageSelected(new Set(['a', 'b']), ['a', 'b'], false)]).toEqual([]);
  });
});

describe('selectedRows', () => {
  it('keeps the page order and drops ids no longer on the page', () => {
    const rows = [{ id: 'a' }, { id: 'b' }, { id: 'c' }];
    expect(selectedRows(rows, new Set(['c', 'a', 'gone']))).toEqual([{ id: 'a' }, { id: 'c' }]);
  });
});
