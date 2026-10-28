import { describe, expect, it } from 'vitest';
import type { BudgetGoal, CashflowGroup, CategorySpendingSummary } from '@shared/schema';
import {
  expenseRows,
  filterCounts,
  filterRows,
  limitState,
  sortRows,
  type ExpenseRow,
} from './cashflow-budget-rows';

const group = (key: string, amount: string, count: number, name = ''): CashflowGroup => ({
  key,
  name,
  flowKind: 'expense',
  amount,
  count,
});

const goal = (categoryId: string, amount = '1000', currency = 'CZK'): BudgetGoal => ({
  id: `g-${categoryId}`,
  categoryId,
  timeframe: 'monthly',
  amount,
  currency,
  createdAt: 0,
  updatedAt: 0,
  effectiveMonth: 0,
  versions: [],
});

const summary = (
  categoryId: string,
  fields: Partial<CategorySpendingSummary> = {}
): CategorySpendingSummary => ({
  budgetCurrency: null,
  budgetSpent: null,
  budgetLimitNative: null,
  budgetLimit: null,
  budgetLimitFxQuality: null,
  categoryId,
  categoryName: categoryId,
  categoryIcon: null,
  categoryColor: null,
  totalAmount: '0.00',
  transactionCount: 0,
  budgetGoal: null,
  budgetPercentage: null,
  ...fields,
});

const limited = (
  categoryId: string,
  spent: string,
  limit: string,
  currency = 'CZK',
  fields: Partial<CategorySpendingSummary> = {}
) =>
  summary(categoryId, {
    budgetGoal: goal(categoryId, limit, currency),
    budgetCurrency: currency,
    budgetSpent: spent,
    budgetLimitNative: limit,
    budgetPercentage: (Number(spent) / Number(limit)) * 100,
    ...fields,
  });

const byKey = (rows: ExpenseRow[]) => new Map(rows.map((row) => [row.key, row]));

describe('expenseRows', () => {
  it('joins the two reports by category id and counts each transaction once', () => {
    const rows = expenseRows(
      [group('cat_groceries', '900.00', 4, 'Groceries')],
      [
        limited('cat_groceries', '900.00', '1000.00', 'CZK', {
          totalAmount: '900.00',
          transactionCount: 4,
          categoryName: 'Potraviny',
        }),
      ]
    );
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ key: 'cat_groceries', amount: 900, count: 4, hasLimit: true });
  });

  it('keeps two categories apart even when their names are the same', () => {
    const rows = expenseRows(
      [group('a', '100.00', 1, 'Food'), group('b', '50.00', 1, 'Food')],
      [limited('b', '50.00', '100.00')]
    );
    expect(rows.map((row) => row.key)).toEqual(['a', 'b']);
    expect(byKey(rows).get('a')?.hasLimit).toBe(false);
    expect(byKey(rows).get('b')?.hasLimit).toBe(true);
  });

  it('lists a category with a limit and no spending', () => {
    const rows = expenseRows(
      [group('cat_dining', '300.00', 2)],
      [limited('cat_health', '0.00', '500.00')]
    );
    expect(byKey(rows).get('cat_health')).toMatchObject({
      amount: 0,
      count: 0,
      hasLimit: true,
      share: 0,
    });
    expect(byKey(rows).get('cat_health')?.limit?.state).toBe('ok');
  });

  it('takes the count of a budget-only line from the budgeting report', () => {
    // Only unconverted rows: no cashflow group, but the rows exist.
    const rows = expenseRows([], [limited('x', '0.00', '500.00', 'CZK', { transactionCount: 2 })]);
    expect(rows[0].count).toBe(2);
    // Its amount is unknown, not a zero spend.
    expect(rows[0].amountUnavailable).toBe(true);
    expect(expenseRows([], [limited('y', '0.00', '500.00')])[0].amountUnavailable).toBe(false);
    expect(expenseRows([group('z', '10.00', 1)], [])[0].amountUnavailable).toBe(false);
  });

  it('keeps spending without a limit and uncategorized rows', () => {
    const rows = expenseRows(
      [group('cat_dining', '300.00', 2), group('uncategorized', '120.00', 1)],
      []
    );
    expect(rows.map((row) => [row.key, row.hasLimit, row.limit])).toEqual([
      ['cat_dining', false, null],
      ['uncategorized', false, null],
    ]);
  });

  it('drops an empty group but keeps a zero net with transactions', () => {
    const rows = expenseRows([group('empty', '0.00', 0), group('netted', '0.00', 2)], []);
    expect(rows.map((row) => row.key)).toEqual(['netted']);
  });

  it('shares absolute movement, so a net refund has a positive share', () => {
    const rows = expenseRows(
      [group('a', '300.00', 1), group('refund', '-100.00', 2), group('b', '600.00', 1)],
      []
    );
    const map = byKey(rows);
    expect(map.get('refund')?.amount).toBe(-100);
    expect(map.get('refund')?.share).toBeCloseTo(0.1);
    expect(map.get('a')?.share).toBeCloseTo(0.3);
    expect(rows.reduce((sum, row) => sum + row.share, 0)).toBeCloseTo(1);
  });

  it('has zero shares without any movement', () => {
    const rows = expenseRows([group('netted', '0.00', 2)], []);
    expect(rows[0].share).toBe(0);
  });

  it('marks a limit that cannot be evaluated as unavailable, never as no limit', () => {
    const rows = expenseRows(
      [group('a', '100.00', 1), group('b', '100.00', 1)],
      [
        limited('a', '100.00', '1000.00', 'EUR', { budgetSpent: null, budgetPercentage: null }),
        limited('b', '100.00', '1000.00', 'EUR', { budgetLimitNative: null }),
      ]
    );
    for (const row of rows) {
      expect(row.hasLimit).toBe(true);
      expect(row.limit?.state).toBe('unavailable');
      expect(row.limit?.remaining).toBeNull();
    }
    expect(byKey(rows).get('a')?.limit?.limit).toBe(1000);
    expect(byKey(rows).get('b')?.limit?.limit).toBeNull();
  });
});

describe('limitState', () => {
  const state = (spent: string, limit: string, currency = 'CZK') =>
    limitState(limited('c', spent, limit, currency));

  it('reads 85 % as near and just below it as fine', () => {
    expect(state('850.00', '1000.00').state).toBe('near');
    expect(state('849.99', '1000.00').state).toBe('ok');
  });

  it('reads an exactly used limit as exhausted, not over', () => {
    const exact = state('1000.00', '1000.00');
    expect(exact.state).toBe('exhausted');
    expect(exact.remaining).toBe(0);
    // 1/3 of a limit three times: the float percentage is not exactly 100.
    expect(
      limitState(limited('c', '300', '300', 'JPY', { budgetPercentage: (0.1 + 0.2) * (100 / 0.3) }))
        .state
    ).toBe('exhausted');
  });

  it('reads anything past the limit as over, with the overshoot as a negative remainder', () => {
    const over = state('1000.01', '1000.00');
    expect(over.state).toBe('over');
    expect(over.remaining).toBeCloseTo(-0.01);
  });

  it('computes the remainder in the minor units of the budget currency', () => {
    expect(state('0.1', '0.3', 'EUR').remaining).toBe(0.2);
    expect(state('100', '1000', 'JPY').remaining).toBe(900);
    expect(state('1.001', '2.000', 'KWD').remaining).toBe(0.999);
  });

  it('treats a net refund as spending below zero, not as unavailable', () => {
    const refund = state('-50.00', '1000.00');
    expect(refund.state).toBe('ok');
    expect(refund.remaining).toBe(1050);
  });

  it('is unavailable without a finite percentage, a currency or a native amount', () => {
    expect(limitState(limited('c', '1', '2', 'CZK', { budgetPercentage: Number.NaN })).state).toBe(
      'unavailable'
    );
    expect(limitState(limited('c', '1', '2', 'CZK', { budgetCurrency: null })).state).toBe(
      'unavailable'
    );
    expect(limitState(limited('c', 'x', '2', 'CZK')).state).toBe('unavailable');
  });

  it('does not depend on the report currency: only native values decide', () => {
    const native = limited('c', '90.00', '100.00', 'EUR', { totalAmount: '2250.00' });
    const switched = { ...native, totalAmount: '97.30' };
    expect(limitState(native)).toEqual(limitState(switched));
  });
});

describe('filters and sorting', () => {
  const rows = expenseRows(
    [
      group('cat_a', '500.00', 3),
      group('cat_b', '1500.00', 5),
      group('uncategorized', '200.00', 1),
      group('cat_c', '500.00', 2),
    ],
    [
      limited('cat_a', '500.00', '400.00'),
      limited('cat_b', '1500.00', '2000.00'),
      limited('cat_d', '0.00', '300.00'),
      limited('cat_e', '10.00', '100.00', 'EUR', { budgetSpent: null, budgetPercentage: null }),
    ]
  );
  const names: Record<string, string> = {
    cat_a: 'Čaj',
    cat_b: 'Auto',
    cat_c: 'Cesty',
    cat_d: 'Dárky',
    cat_e: 'Zdraví',
    uncategorized: 'Nezařazeno',
  };
  const label = (row: ExpenseRow) => names[row.key];

  it('counts every filter over the whole overview', () => {
    expect(filterCounts(rows)).toEqual({ all: 6, over: 1, none: 2 });
    expect(filterRows(rows, 'over').map((row) => row.key)).toEqual(['cat_a']);
    expect(filterRows(rows, 'none').map((row) => row.key)).toEqual(['uncategorized', 'cat_c']);
  });

  it('keeps an unavailable limit out of "without a limit"', () => {
    expect(filterRows(rows, 'none').some((row) => row.key === 'cat_e')).toBe(false);
  });

  it('can filter to nothing', () => {
    expect(filterRows(filterRows(rows, 'none'), 'over')).toEqual([]);
  });

  it('sorts by amount with a deterministic tie-break', () => {
    expect(sortRows(rows, 'amount', label, 'cs').map((row) => row.key)).toEqual([
      'cat_b',
      'cat_a',
      'cat_c',
      'uncategorized',
      'cat_d',
      'cat_e',
    ]);
  });

  it('sorts by usage: evaluated first, then unavailable, then rows without a limit', () => {
    expect(sortRows(rows, 'usage', label, 'cs').map((row) => row.key)).toEqual([
      'cat_a',
      'cat_b',
      'cat_d',
      'cat_e',
      'cat_c',
      'uncategorized',
    ]);
  });

  it('sorts by name with the locale (Č after C in Czech)', () => {
    expect(sortRows(rows, 'name', label, 'cs').map(label)).toEqual([
      'Auto',
      'Cesty',
      'Čaj',
      'Dárky',
      'Nezařazeno',
      'Zdraví',
    ]);
  });

  it('never changes the rows themselves', () => {
    const before = rows.map((row) => row.key);
    sortRows(rows, 'name', label, 'cs');
    expect(rows.map((row) => row.key)).toEqual(before);
  });
});
