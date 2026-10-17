import { describe, expect, it } from 'vitest';
import type {
  BudgetGoal,
  CashflowGroup,
  CashflowMonth,
  CategorySpendingSummary,
} from '@shared/schema';
import { barsFor, budgetCounts, categoryRows, savingsRate } from './cashflow-rows';

function group(patch: Partial<CashflowGroup> & Pick<CashflowGroup, 'key'>): CashflowGroup {
  return { name: '', flowKind: 'expense', amount: '0', count: 0, ...patch };
}

const GOAL: BudgetGoal = {
  id: 'goal',
  categoryId: 'cat_groceries',
  timeframe: 'monthly',
  amount: '8000',
  currency: 'CZK',
  createdAt: 0,
  updatedAt: 0,
  effectiveMonth: null,
  versions: [],
};

function summary(
  categoryId: string,
  patch: Partial<CategorySpendingSummary> = {}
): CategorySpendingSummary {
  return {
    budgetCurrency: 'CZK',
    budgetSpent: '7820',
    budgetLimitNative: '8000',
    budgetLimit: '8000',
    budgetLimitFxQuality: null,
    categoryId,
    categoryName: categoryId,
    categoryIcon: null,
    categoryColor: null,
    totalAmount: '7820',
    transactionCount: 4,
    budgetGoal: { ...GOAL, categoryId },
    budgetPercentage: 97.75,
    ...patch,
  };
}

function budgetMap(...summaries: CategorySpendingSummary[]) {
  return new Map(summaries.map((s) => [s.categoryId, s]));
}

describe('categoryRows', () => {
  const groups = [
    group({ key: 'cat_housing', amount: '12500', count: 3 }),
    group({ key: 'cat_energy', amount: '1900', count: 2 }),
    group({ key: 'cat_zero', amount: '0', count: 0 }),
  ];

  it('drops empty groups and shares the rest by absolute movement', () => {
    const rows = categoryRows(groups);
    expect(rows.map((row) => row.key)).toEqual(['cat_housing', 'cat_energy']);
    expect(rows[0].share).toBeCloseTo(12500 / 14400);
    expect(rows[1].share).toBeCloseTo(1900 / 14400);
    expect(rows.every((row) => row.limit === null)).toBe(true);
  });

  it('keeps the order, the name, the kind, the amount and the count of each group', () => {
    const [first] = categoryRows([
      group({ key: 'cat_x', name: 'Bydlení', flowKind: 'transfer', amount: '-250.5', count: 7 }),
    ]);
    expect(first).toMatchObject({
      key: 'cat_x',
      name: 'Bydlení',
      flowKind: 'transfer',
      amount: -250.5,
      count: 7,
      share: 1,
    });
  });

  it('keeps a group that has transactions but nets to zero', () => {
    expect(categoryRows([group({ key: 'cat_net', amount: '0', count: 2 })])).toHaveLength(1);
  });

  it('keeps a group with an amount but no counted rows', () => {
    expect(categoryRows([group({ key: 'cat_fx', amount: '40', count: 0 })])).toHaveLength(1);
  });

  it('gives no shares to a list that moves nothing', () => {
    const rows = categoryRows([group({ key: 'cat_net', amount: '0', count: 2 })]);
    expect(rows[0].share).toBe(0);
  });

  describe('with budgets', () => {
    const groceries = group({ key: 'cat_groceries', amount: '7820', count: 12 });

    it('reads a limit that is 97.75 % spent as near', () => {
      const [row] = categoryRows([groceries], budgetMap(summary('cat_groceries')));
      expect(row.limit).toEqual({
        spent: 7820,
        limit: 8000,
        currency: 'CZK',
        percentage: 97.75,
        state: 'near',
      });
    });

    it('reads a limit that is exceeded as over', () => {
      const [row] = categoryRows(
        [groceries],
        budgetMap(summary('cat_groceries', { budgetPercentage: 115.3 }))
      );
      expect(row.limit?.state).toBe('over');
    });

    it('changes state at 85 % (near) and above 100 % (over)', () => {
      const state = (percentage: number) =>
        categoryRows(
          [groceries],
          budgetMap(summary('cat_groceries', { budgetPercentage: percentage }))
        )[0].limit?.state;
      expect(state(40)).toBe('ok');
      expect(state(84.99)).toBe('ok');
      expect(state(85)).toBe('near');
      expect(state(100)).toBe('near');
      expect(state(100.01)).toBe('over');
    });

    it('keeps the native numbers of a limit in another currency next to the report-currency amount', () => {
      const [row] = categoryRows(
        [groceries],
        budgetMap(
          summary('cat_groceries', {
            budgetCurrency: 'EUR',
            budgetSpent: '290',
            budgetLimitNative: '300',
            budgetPercentage: 96.7,
          })
        )
      );
      expect(row.amount).toBe(7820);
      expect(row.limit).toMatchObject({ currency: 'EUR', spent: 290, limit: 300 });
    });

    it('has no limit when the summary cannot be evaluated', () => {
      const unavailable = [
        summary('cat_groceries', { budgetLimitNative: null }),
        summary('cat_groceries', { budgetSpent: null }),
        summary('cat_groceries', { budgetCurrency: null }),
        summary('cat_groceries', { budgetPercentage: null }),
        summary('cat_groceries', { budgetPercentage: Number.NaN }),
        summary('cat_groceries', { budgetPercentage: Number.POSITIVE_INFINITY }),
        summary('cat_groceries', { budgetGoal: null }),
      ];
      for (const one of unavailable) {
        expect(categoryRows([groceries], budgetMap(one))[0].limit).toBeNull();
      }
    });

    it('has no limit for a category without a summary', () => {
      const [row] = categoryRows([groceries], budgetMap(summary('cat_other')));
      expect(row.limit).toBeNull();
    });
  });
});

describe('budgetCounts', () => {
  it('is null without summaries', () => {
    expect(budgetCounts([])).toBeNull();
  });

  it('is null when no summary has a budget goal', () => {
    expect(
      budgetCounts([summary('a', { budgetGoal: null }), summary('b', { budgetGoal: null })])
    ).toBeNull();
  });

  it('is null when no goal can be evaluated', () => {
    expect(budgetCounts([summary('a', { budgetLimitNative: null })])).toBeNull();
  });

  it('counts the goals by state and leaves out the unavailable and the goalless', () => {
    expect(
      budgetCounts([
        summary('ok', { budgetPercentage: 40 }),
        summary('near', { budgetPercentage: 90 }),
        summary('over', { budgetPercentage: 130 }),
        summary('unavailable', { budgetLimitNative: null }),
        summary('goalless', { budgetGoal: null, budgetPercentage: 150 }),
      ])
    ).toEqual({ ok: 1, near: 1, over: 1 });
  });
});

describe('barsFor', () => {
  const months: CashflowMonth[] = Array.from({ length: 12 }, (_, index) => ({
    month: Date.UTC(2025, 10 + index, 1) / 1000,
    income: String(1000 + index),
    expenses: String(500 + index),
  }));
  const septemberStart = Date.UTC(2026, 8, 1) / 1000;
  const septemberEnd = Date.UTC(2026, 8, 30, 23, 59, 59) / 1000;

  it('mutes every month outside a monthly period', () => {
    const bars = barsFor(months, septemberStart, septemberEnd);
    expect(bars).toHaveLength(12);
    expect(bars.filter((bar) => !bar.muted).map((bar) => bar.month)).toEqual([septemberStart]);
  });

  it('leaves the months of a quarter unmuted', () => {
    const quarterStart = Date.UTC(2026, 6, 1) / 1000;
    const quarterEnd = Date.UTC(2026, 9, 1) / 1000 - 1;
    const bars = barsFor(months, quarterStart, quarterEnd);
    expect(
      bars.filter((bar) => !bar.muted).map((bar) => new Date(bar.month * 1000).getUTCMonth())
    ).toEqual([6, 7, 8]);
  });

  it('reads the amounts as numbers', () => {
    const [first] = barsFor(months, septemberStart, septemberEnd);
    expect(first).toEqual({ month: months[0].month, income: 1000, expenses: 500, muted: true });
  });
});

describe('savingsRate', () => {
  it('is the net over the income', () => {
    expect(savingsRate(41200, 9870)).toBeCloseTo(0.2396, 4);
    expect(savingsRate(1000, -250)).toBeCloseTo(-0.25);
  });

  it('is null without income', () => {
    expect(savingsRate(0, -5)).toBeNull();
    expect(savingsRate(-10, 5)).toBeNull();
  });
});
