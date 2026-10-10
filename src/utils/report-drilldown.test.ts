import { describe, expect, it } from 'vitest';
import { applyBulkChanges, isMovedOut, movedCount, restoreBulkChanges } from './report-drilldown';

describe('isMovedOut', () => {
  it('flags a row whose category no longer matches the dialog target', () => {
    expect(isMovedOut('cat_dining', 'cat_dining')).toBe(false);
    expect(isMovedOut('cat_dining', 'cat_groceries')).toBe(true);
    expect(isMovedOut('cat_dining', null)).toBe(true);
  });

  it('treats uncategorized as its own target', () => {
    expect(isMovedOut('uncategorized', null)).toBe(false);
    expect(isMovedOut('uncategorized', 'cat_other_income')).toBe(true);
  });

  it('never flags rows of a dialog without a category target', () => {
    expect(isMovedOut(undefined, 'cat_dining')).toBe(false);
    expect(isMovedOut(undefined, null)).toBe(false);
  });
});

describe('movedCount', () => {
  it('counts changed rows that left the target, not rows moved back', () => {
    const rows = [
      { id: 'a', categoryId: null },
      { id: 'b', categoryId: null },
      { id: 'c', categoryId: null },
    ];
    const changes = new Map<string, string | null>([
      ['a', 'cat_dining'],
      ['b', null],
    ]);
    expect(movedCount(rows, changes, 'uncategorized')).toBe(1);
    expect(movedCount(rows, new Map(), 'uncategorized')).toBe(0);
  });
});

describe('bulk changes from the picker', () => {
  const rows = [
    { id: 'a', categoryId: 'cat_dining' },
    { id: 'b', categoryId: 'cat_dining' },
    { id: 'c', categoryId: null },
  ];
  const changed = [
    { id: 'a', previousCategoryId: 'cat_dining', previousSource: null },
    { id: 'c', previousCategoryId: null, previousSource: null },
    { id: 'elsewhere', previousCategoryId: 'cat_dining', previousSource: 'rule' },
  ];

  it('marks only the rows the dialog holds with the new category', () => {
    const next = applyBulkChanges(new Map(), rows, changed, 'cat_groceries');
    expect([...next.entries()]).toEqual([
      ['a', 'cat_groceries'],
      ['c', 'cat_groceries'],
    ]);
  });

  it('keeps earlier single-row changes', () => {
    const next = applyBulkChanges(new Map([['b', 'cat_gifts']]), rows, changed, 'cat_groceries');
    expect(next.get('b')).toBe('cat_gifts');
    expect(next.get('a')).toBe('cat_groceries');
  });

  it('restores a row to its previous category, dropping the mark when that is the original', () => {
    const applied = applyBulkChanges(new Map(), rows, changed, 'cat_groceries');
    const restored = restoreBulkChanges(applied, rows, changed);
    expect(restored.has('a')).toBe(false);
    expect(restored.has('c')).toBe(false);
    expect(restored.has('elsewhere')).toBe(false);
  });

  it('keeps a mark when the previous category differs from the original', () => {
    const earlier = [{ id: 'b', previousCategoryId: 'cat_gifts', previousSource: null }];
    const restored = restoreBulkChanges(new Map([['b', 'cat_groceries']]), rows, earlier);
    expect(restored.get('b')).toBe('cat_gifts');
  });
});
