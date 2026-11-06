import { describe, expect, it } from 'vitest';
import type { CategoryUsage, TransactionCategory } from '@shared/schema';
import {
  canConfirmDelete,
  categoryRowActions,
  groupCategories,
  reassignTargets,
  usageTotal,
  type CategoryRow,
} from './category-rows';

const cat = (id: string, kind: TransactionCategory['kind'], isSystem = false) =>
  ({
    id,
    kind,
    name: id,
    icon: null,
    color: null,
    parentId: null,
    sortOrder: 0,
    isSystem,
    createdAt: 0,
  }) satisfies TransactionCategory;

const use = (categoryId: string, patch: Partial<CategoryUsage> = {}): CategoryUsage => ({
  categoryId,
  transactions: 0,
  rules: 0,
  learnedPayees: 0,
  budgetGoals: 0,
  ...patch,
});

describe('groupCategories', () => {
  it('orders the kinds expense, income, transfer and keeps the in-group order', () => {
    const groups = groupCategories(
      [cat('t', 'transfer'), cat('i1', 'income'), cat('e1', 'expense'), cat('e2', 'expense')],
      []
    );
    expect(groups.map((g) => g.kind)).toEqual(['expense', 'income', 'transfer']);
    expect(groups[0].rows.map((r) => r.category.id)).toEqual(['e1', 'e2']);
  });

  it('omits a kind with no categories', () => {
    expect(groupCategories([cat('e1', 'expense')], []).map((g) => g.kind)).toEqual(['expense']);
  });

  it('gives a row without usage usage null and not in use', () => {
    const [group] = groupCategories([cat('e1', 'expense')], []);
    expect(group.rows[0]).toMatchObject({ usage: null, inUse: false });
  });

  it('counts a category with only a budget goal as in use', () => {
    const [group] = groupCategories([cat('e1', 'expense')], [use('e1', { budgetGoals: 1 })]);
    expect(group.rows[0].inUse).toBe(true);
  });

  it('a zero usage row is not in use', () => {
    const [group] = groupCategories([cat('e1', 'expense')], [use('e1')]);
    expect(group.rows[0].inUse).toBe(false);
  });
});

describe('usageTotal', () => {
  it('sums the four counts', () => {
    expect(
      usageTotal(use('x', { transactions: 1, rules: 2, learnedPayees: 3, budgetGoals: 4 }))
    ).toBe(10);
  });
});

describe('reassignTargets', () => {
  it('never contains the deleted category and lists its kind first', () => {
    const all = [
      cat('i1', 'income'),
      cat('e1', 'expense'),
      cat('e2', 'expense'),
      cat('t', 'transfer'),
    ];
    const targets = reassignTargets(all, all[1]);
    expect(targets.map((c) => c.id)).toEqual(['e2', 'i1', 't']);
  });
});

describe('canConfirmDelete', () => {
  const row = (inUse: boolean): CategoryRow => ({
    category: cat('x', 'expense'),
    usage: null,
    inUse,
  });
  it('needs a target for a category in use', () => {
    expect(canConfirmDelete(row(true), null)).toBe(false);
    expect(canConfirmDelete(row(true), 'x')).toBe(true);
  });
  it('lets an unused category go without a target', () => {
    expect(canConfirmDelete(row(false), null)).toBe(true);
  });
});

describe('categoryRowActions', () => {
  it('never offers delete for a system category', () => {
    const row: CategoryRow = { category: cat('s', 'expense', true), usage: null, inUse: false };
    expect(categoryRowActions(row)).toEqual({ edit: true, delete: false });
  });
  it('offers delete for a custom one', () => {
    const row: CategoryRow = { category: cat('c', 'expense'), usage: null, inUse: false };
    expect(categoryRowActions(row).delete).toBe(true);
  });
});
