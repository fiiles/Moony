import { describe, expect, it } from 'vitest';
import type { LearnedPayeeEntry } from '@shared/schema';
import {
  ALL_CATEGORIES,
  countRulesByCategory,
  filterLearnedRules,
  paginate,
  selectionState,
} from './learned-rules';

function rule(id: string, overrides: Partial<LearnedPayeeEntry> = {}): LearnedPayeeEntry {
  return {
    id,
    ruleType: 'payee_default',
    originalPayee: `Payee ${id}`,
    normalizedPayee: `payee ${id}`,
    categoryId: 'cat_food',
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

const names: Record<string, string> = { cat_food: 'Groceries', cat_travel: 'Travel' };
const nameOf = (id: string) => names[id];

describe('filterLearnedRules', () => {
  const rules = [
    rule('1', { originalPayee: 'LIDL Praha', normalizedPayee: 'lidl praha' }),
    rule('2', {
      originalPayee: 'Austrian Air',
      normalizedPayee: 'austrian air',
      categoryId: 'cat_travel',
    }),
    rule('3', {
      originalPayee: undefined,
      normalizedPayee: undefined,
      counterpartyIban: 'CZ6508000000192000145399',
      ruleType: 'iban_only_default',
    }),
  ];

  it('keeps everything without a query or category', () => {
    expect(
      filterLearnedRules(rules, { query: '', categoryId: ALL_CATEGORIES }, nameOf)
    ).toHaveLength(3);
  });

  it('searches payee, IBAN and category name case-insensitively', () => {
    const ids = (query: string) =>
      filterLearnedRules(rules, { query, categoryId: ALL_CATEGORIES }, nameOf).map((r) => r.id);
    expect(ids('lidl')).toEqual(['1']);
    expect(ids('  AUSTRIAN ')).toEqual(['2']);
    expect(ids('cz6508')).toEqual(['3']);
    expect(ids('travel')).toEqual(['2']);
    expect(ids('nothing like this')).toEqual([]);
  });

  it('filters by category and combines it with the search', () => {
    const ids = (query: string, categoryId: string) =>
      filterLearnedRules(rules, { query, categoryId }, nameOf).map((r) => r.id);
    expect(ids('', 'cat_travel')).toEqual(['2']);
    expect(ids('lidl', 'cat_travel')).toEqual([]);
    expect(ids('lidl', 'cat_food')).toEqual(['1']);
  });
});

describe('countRulesByCategory', () => {
  it('counts per category and leaves out empty ones', () => {
    const counts = countRulesByCategory([
      rule('1'),
      rule('2'),
      rule('3', { categoryId: 'cat_travel' }),
    ]);
    expect(counts.get('cat_food')).toBe(2);
    expect(counts.get('cat_travel')).toBe(1);
    expect(counts.has('cat_other')).toBe(false);
  });
});

describe('paginate', () => {
  const items = Array.from({ length: 519 }, (_, i) => i);

  it('slices pages and reports the range', () => {
    const first = paginate(items, 0, 50);
    expect(first.items).toHaveLength(50);
    expect([first.from, first.to, first.total, first.pageCount]).toEqual([1, 50, 519, 11]);

    const last = paginate(items, 10, 50);
    expect(last.items).toHaveLength(19);
    expect([last.from, last.to]).toEqual([501, 519]);
  });

  it('clamps an out-of-range page (the filters shrank the list)', () => {
    const page = paginate(items.slice(0, 60), 9, 50);
    expect(page.page).toBe(1);
    expect(page.items).toHaveLength(10);
    expect(paginate(items, -3, 50).page).toBe(0);
  });

  it('handles an empty list as one empty page', () => {
    expect(paginate([], 0, 50)).toEqual({
      items: [],
      page: 0,
      pageCount: 1,
      total: 0,
      from: 0,
      to: 0,
    });
  });

  it('honours the larger page sizes', () => {
    expect(paginate(items, 0, 200).items).toHaveLength(200);
    expect(paginate(items, 0, 200).pageCount).toBe(3);
  });
});

describe('selectionState', () => {
  it('is tri-state over the given ids', () => {
    const selected = new Set(['a', 'b', 'x']);
    expect(selectionState(['a', 'b'], selected)).toBe('all');
    expect(selectionState(['a', 'c'], selected)).toBe('some');
    expect(selectionState(['c', 'd'], selected)).toBe('none');
    expect(selectionState([], selected)).toBe('none');
  });
});
