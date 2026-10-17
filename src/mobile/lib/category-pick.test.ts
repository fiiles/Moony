import { describe, expect, it } from 'vitest';
import { filterCategories, learnIdentity, pickOutcome, recentCategoryIds } from './category-pick';

describe('recentCategoryIds', () => {
  it("lists the newest rows' categories once each, newest first, up to the limit", () => {
    const rows = [
      { categoryId: 'a', bookingDate: 30 },
      { categoryId: null, bookingDate: 29 },
      { categoryId: 'b', bookingDate: 28 },
      { categoryId: 'a', bookingDate: 27 },
      { categoryId: 'c', bookingDate: 26 },
    ];
    expect(recentCategoryIds(rows, 2)).toEqual(['a', 'b']);
    expect(recentCategoryIds([], 5)).toEqual([]);
  });
});

describe('filterCategories', () => {
  const cats = [
    { id: '1', name: 'Potraviny', parentId: null, sortOrder: 2 },
    { id: '2', name: 'Doprava', parentId: null, sortOrder: 1 },
    { id: '3', name: 'Sub', parentId: '1', sortOrder: 0 },
  ];
  const nameOf = (c: { name: string }) => c.name;
  it('keeps top-level categories in sort order and matches the query without accents or case', () => {
    expect(filterCategories(cats, '', nameOf).map((c) => c.id)).toEqual(['2', '1']);
    expect(filterCategories(cats, 'POTRA', nameOf).map((c) => c.id)).toEqual(['1']);
    expect(filterCategories(cats, 'dopr', nameOf).map((c) => c.id)).toEqual(['2']);
  });
});

describe('learnIdentity', () => {
  it('needs a payee longer than two characters or an account longer than five', () => {
    expect(learnIdentity({ counterpartyName: ' Lidl ', counterpartyIban: null })).toEqual({
      payee: 'Lidl',
      iban: null,
    });
    expect(learnIdentity({ counterpartyName: 'AB', counterpartyIban: 'CZ65' })).toBeNull();
    expect(
      learnIdentity({ counterpartyName: null, counterpartyIban: 'CZ6508000000192000145399' })
    ).toEqual({ payee: null, iban: 'CZ6508000000192000145399' });
  });
});

describe('pickOutcome', () => {
  const preview = (
    p: Partial<{ governing: string; learnable: boolean; rule: { id: string } | null }>
  ) =>
    ({ governing: 'none', learnable: true, rule: null, matches: 0, byCategory: [], ...p }) as never;
  it('skips without remembering, on an own-account transfer and when nothing is learnable', () => {
    expect(pickOutcome(preview({}), false, true)).toBe('skip');
    expect(pickOutcome(preview({ governing: 'ownAccount' }), true, true)).toBe('skip');
    expect(pickOutcome(preview({ learnable: false }), true, true)).toBe('skip');
    expect(pickOutcome(preview({}), true, false)).toBe('skip');
  });
  it('retargets a governing custom rule, learns otherwise', () => {
    expect(pickOutcome(preview({ governing: 'custom', rule: { id: 'r' } }), true, true)).toBe(
      'retarget'
    );
    expect(
      pickOutcome(preview({ governing: 'custom', rule: { id: 'r' }, learnable: false }), true, true)
    ).toBe('retarget');
    expect(pickOutcome(preview({ governing: 'learned' }), true, true)).toBe('learn');
    expect(pickOutcome(undefined, true, true)).toBe('learn');
  });
});
