import { describe, expect, it } from 'vitest';
import { isMovedOut, movedCount } from './report-drilldown';

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
