import { describe, expect, it } from 'vitest';
import type { BankAccount, CashflowActuals, CashflowGroup, CashflowMonth } from '@shared/schema';
import {
  accountScope,
  barsFor,
  bodyKind,
  categoryRows,
  categoryTitle,
  savingsRate,
  UNCATEGORIZED,
} from './cashflow-rows';

function group(patch: Partial<CashflowGroup> & Pick<CashflowGroup, 'key'>): CashflowGroup {
  return { name: '', flowKind: 'expense', amount: '0', count: 0, ...patch };
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

describe('categoryTitle', () => {
  const labels = {
    categoryName: (category: { id: string; name: string; isSystem?: boolean }) =>
      `${category.isSystem ? 'system' : 'custom'}:${category.name}`,
    uncategorized: 'Bez kategorie',
    uncategorizedTransfers: 'Převody bez kategorie',
  };

  it('titles the uncategorized group by its flow', () => {
    expect(categoryTitle(UNCATEGORIZED, 'expense', null, '', labels)).toBe('Bez kategorie');
    expect(categoryTitle(UNCATEGORIZED, 'income', null, '', labels)).toBe('Bez kategorie');
    expect(categoryTitle(UNCATEGORIZED, 'transfer', null, '', labels)).toBe(
      'Převody bez kategorie'
    );
  });

  it('names a known category as the category itself', () => {
    const category = { id: 'cat_groceries', name: 'Groceries', isSystem: false };
    expect(categoryTitle('cat_groceries', 'expense', category, 'Stored', labels)).toBe(
      'custom:Groceries'
    );
  });

  it('names an unknown category by the stored name, a system one by its cat_ id', () => {
    expect(categoryTitle('cat_travel', 'expense', undefined, 'Travel', labels)).toBe(
      'system:Travel'
    );
    expect(categoryTitle('1b2c', 'expense', null, 'Hobby', labels)).toBe('custom:Hobby');
  });
});

describe('bodyKind', () => {
  function actuals(patch: Partial<CashflowActuals> = {}): CashflowActuals {
    return {
      reportCurrency: 'CZK',
      periodStart: 0,
      periodEnd: 0,
      quality: {
        missingFxCount: 0,
        missingFxCurrencies: [],
        estimatedFxCount: 0,
        observedCompletedMonths: [],
        partialMonthCount: 0,
        unobservedMonthCount: 0,
        invalidAmountCount: 0,
      },
      months: [],
      totalIncome: '0',
      totalExpenses: '0',
      netBalance: '0',
      bankCashMovement: '0',
      averageIncome: null,
      averageExpenses: null,
      averageCompletedMonths: [],
      expenseCategories: [],
      incomeCategories: [],
      transfers: [],
      accountCount: 2,
      transactionCount: 14,
      ...patch,
    };
  }
  const loaded = { accountsLoaded: true, accountTotal: 2, isError: false, hasLimits: false };

  it('is the skeleton while the actuals are not there yet', () => {
    expect(bodyKind({ ...loaded, actuals: undefined })).toBe('skeleton');
  });

  it('is the skeleton while the accounts are not loaded, whatever the actuals say', () => {
    expect(bodyKind({ ...loaded, accountsLoaded: false, actuals: actuals() })).toBe('skeleton');
    expect(
      bodyKind({
        accountsLoaded: false,
        accountTotal: 0,
        isError: false,
        hasLimits: false,
        actuals: actuals({ transactionCount: 0, accountCount: 0 }),
      })
    ).toBe('skeleton');
  });

  it('is the error when the actuals failed and there are none to show', () => {
    expect(bodyKind({ ...loaded, isError: true, actuals: undefined })).toBe('error');
    expect(
      bodyKind({
        accountsLoaded: false,
        accountTotal: 0,
        isError: true,
        hasLimits: false,
        actuals: undefined,
      })
    ).toBe('error');
  });

  it('keeps the report when a refetch failed but the actuals are still there', () => {
    expect(bodyKind({ ...loaded, isError: true, actuals: actuals() })).toBe('report');
  });

  it('is the no-accounts invitation only for a profile without any account', () => {
    expect(
      bodyKind({
        accountsLoaded: true,
        accountTotal: 0,
        isError: false,
        hasLimits: false,
        actuals: actuals({ transactionCount: 0, accountCount: 0 }),
      })
    ).toBe('noAccounts');
  });

  it('is an empty period, not the invitation, when accounts exist but the period has nothing', () => {
    // the backend counts accounts among the period's own transactions: 0 for any empty period
    expect(
      bodyKind({ ...loaded, actuals: actuals({ transactionCount: 0, accountCount: 0 }) })
    ).toBe('emptyPeriod');
    expect(
      bodyKind({ ...loaded, accountTotal: 1, actuals: actuals({ transactionCount: 0 }) })
    ).toBe('emptyPeriod');
  });

  it('is the report for a period with transactions', () => {
    expect(bodyKind({ ...loaded, actuals: actuals() })).toBe('report');
  });

  it('keeps the report for a period with limits but no transactions (the desktop rule)', () => {
    expect(
      bodyKind({ ...loaded, hasLimits: true, actuals: actuals({ transactionCount: 0 }) })
    ).toBe('report');
  });
});

describe('accountScope', () => {
  const accounts = [{ id: 'a' }, { id: 'b' }, { id: 'c' }] as BankAccount[];

  it('is all accounts without a choice', () => {
    expect(accountScope(undefined, accounts)).toBeNull();
  });

  it('counts the chosen accounts, not the ones with payments in the period', () => {
    expect(accountScope(['a'], accounts)).toBe(1);
    expect(accountScope(['a', 'c'], accounts)).toBe(2);
  });

  it('is all accounts when every account is chosen', () => {
    expect(accountScope(['c', 'b', 'a'], accounts)).toBeNull();
  });

  it('leaves out a chosen account that no longer exists', () => {
    expect(accountScope(['a', 'gone'], accounts)).toBe(1);
  });

  it('counts the choice as it is while the accounts are not loaded', () => {
    expect(accountScope(['a', 'b'], [])).toBe(2);
  });
});
