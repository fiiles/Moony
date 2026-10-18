import { describe, expect, it } from 'vitest';
import type {
  BudgetGoal,
  BudgetingReport,
  CategorySpendingSummary,
  ReportQuality,
} from '@shared/schema';
import {
  averageWindow,
  averagesByCategory,
  limitAmountText,
  limitDefaults,
  limitFormState,
  limitSaveLabel,
} from './budget-limit';

describe('limitFormState', () => {
  it('asks for an amount when the field is empty', () => {
    const state = limitFormState({ amountText: '', from: '2026-10', until: '' });
    expect(state.errors.amount).toBe('required');
    expect(state.amount).toBeNull();
    expect(state.valid).toBe(false);
  });

  it('rejects text that is not a number', () => {
    const state = limitFormState({ amountText: 'abc', from: '2026-10', until: '' });
    expect(state.errors.amount).toBe('invalid');
    expect(state.valid).toBe(false);
  });

  it('rejects zero: a limit has to be greater than nothing', () => {
    const state = limitFormState({ amountText: '0', from: '2026-10', until: '' });
    expect(state.errors.amount).toBe('positive');
    expect(state.valid).toBe(false);
  });

  it('reads a grouped amount and leaves the end open', () => {
    const state = limitFormState({ amountText: '3 000', from: '2026-10', until: '' });
    expect(state.amount).toBe(3000);
    expect(state.from).toBe(Date.UTC(2026, 9, 1) / 1000);
    expect(state.until).toBeNull();
    expect(state.errors).toEqual({});
    expect(state.valid).toBe(true);
  });

  it('reads the comma the system keypad types', () => {
    expect(limitFormState({ amountText: '1 250,5', from: '2026-10', until: '' }).amount).toBe(
      1250.5
    );
  });

  it('refuses an end before the start', () => {
    const state = limitFormState({ amountText: '3000', from: '2026-10', until: '2026-09' });
    expect(state.errors.until).toBe('beforeStart');
    expect(state.until).toBe(Date.UTC(2026, 8, 1) / 1000);
    expect(state.valid).toBe(false);
  });

  it('accepts an end in the same month as the start', () => {
    const state = limitFormState({ amountText: '3000', from: '2026-10', until: '2026-10' });
    expect(state.errors.until).toBeUndefined();
    expect(state.valid).toBe(true);
  });

  it('asks for the start month when it is empty', () => {
    const state = limitFormState({ amountText: '3000', from: '', until: '' });
    expect(state.errors.from).toBe('required');
    expect(state.from).toBeNull();
    expect(state.valid).toBe(false);
  });

  it('calls an end that is no month invalid', () => {
    const state = limitFormState({ amountText: '3000', from: '2026-10', until: '2026-13' });
    expect(state.errors.until).toBe('invalid');
    expect(state.until).toBeNull();
    expect(state.valid).toBe(false);
  });

  it('does not compare an end with a start that is missing', () => {
    const state = limitFormState({ amountText: '3000', from: '', until: '2026-09' });
    expect(state.errors).toEqual({ from: 'required' });
  });
});

describe('limitSaveLabel', () => {
  it('confirms the overwrite on the second tap, whether adding or editing', () => {
    expect(limitSaveLabel(2, true, false)).toBe('overwrite');
    expect(limitSaveLabel(2, true, true)).toBe('overwrite');
  });

  it('says save while editing and add while adding', () => {
    expect(limitSaveLabel(2, false, true)).toBe('save');
    expect(limitSaveLabel(0, true, true)).toBe('save');
    expect(limitSaveLabel(0, true, false)).toBe('add');
    expect(limitSaveLabel(0, false, false)).toBe('add');
  });
});

function quality(patch: Partial<ReportQuality> = {}): ReportQuality {
  return {
    missingFxCount: 0,
    missingFxCurrencies: [],
    estimatedFxCount: 0,
    observedCompletedMonths: [1_790_000_000, 1_792_600_000],
    partialMonthCount: 0,
    unobservedMonthCount: 0,
    invalidAmountCount: 0,
    ...patch,
  };
}

function category(categoryId: string, totalAmount: string): CategorySpendingSummary {
  return {
    budgetCurrency: null,
    budgetSpent: null,
    budgetLimitNative: null,
    budgetLimit: null,
    budgetLimitFxQuality: null,
    categoryId,
    categoryName: categoryId,
    categoryIcon: null,
    categoryColor: null,
    totalAmount,
    transactionCount: 1,
    budgetGoal: null,
    budgetPercentage: null,
  };
}

function report(patch: Partial<BudgetingReport> = {}): BudgetingReport {
  return {
    reportCurrency: 'CZK',
    bankCashMovement: '0',
    quality: quality(),
    nonconsumptionFlows: [],
    periodStart: 0,
    periodEnd: 0,
    timeframe: 'monthly',
    totalIncome: '0',
    totalExpenses: '0',
    netBalance: '0',
    incomeCategories: [],
    expenseCategories: [category('cat_groceries', '16000'), category('cat_fun', '0')],
    uncategorizedIncome: '0',
    uncategorizedExpenses: '0',
    uncategorizedTransactionCount: 0,
    uncategorizedExpenseTransactionCount: 0,
    ...patch,
  };
}

describe('averagesByCategory', () => {
  it('is empty while the report is missing', () => {
    expect(averagesByCategory(undefined).size).toBe(0);
  });

  it('is empty when a rate is missing, an amount is invalid or no month was observed', () => {
    expect(averagesByCategory(report({ quality: quality({ missingFxCount: 1 }) })).size).toBe(0);
    expect(averagesByCategory(report({ quality: quality({ invalidAmountCount: 2 }) })).size).toBe(
      0
    );
    expect(
      averagesByCategory(report({ quality: quality({ observedCompletedMonths: [] }) })).size
    ).toBe(0);
  });

  it('divides each expense category by the months observed', () => {
    const averages = averagesByCategory(report());
    expect(averages.get('cat_groceries')).toEqual({ average: 8000, months: 2 });
    expect(averages.get('cat_fun')).toEqual({ average: 0, months: 2 });
    expect(averages.get('cat_unknown')).toBeUndefined();
  });

  it('reads an unreadable total as no spending', () => {
    const averages = averagesByCategory(
      report({ expenseCategories: [category('cat_groceries', 'n/a')] })
    );
    expect(averages.get('cat_groceries')).toEqual({ average: 0, months: 2 });
  });
});

describe('averageWindow', () => {
  it('spans the six completed months before this one', () => {
    const window = averageWindow(new Date('2026-10-17T00:00:00Z'));
    expect(window.start).toBe(Date.UTC(2026, 3, 1) / 1000);
    expect(window.end).toBe(Date.UTC(2026, 8, 30, 23, 59, 59) / 1000);
  });

  it('crosses the year boundary', () => {
    const window = averageWindow(new Date('2026-02-03T12:00:00Z'));
    expect(window.start).toBe(Date.UTC(2025, 7, 1) / 1000);
    expect(window.end).toBe(Date.UTC(2026, 0, 31, 23, 59, 59) / 1000);
  });
});

describe('limitAmountText', () => {
  it('writes a stored amount as a person would type it', () => {
    expect(limitAmountText('8000.00')).toBe('8000');
    expect(limitAmountText('1250.50')).toBe('1250.5');
    expect(limitAmountText('99.99')).toBe('99.99');
    expect(limitAmountText('8000')).toBe('8000');
    expect(limitAmountText('100')).toBe('100');
  });

  it('leaves anything that is not a plain decimal as it is', () => {
    expect(limitAmountText('1e-7')).toBe('1e-7');
    expect(limitAmountText('')).toBe('');
  });
});

const OCT = Date.UTC(2026, 9, 1) / 1000;
const NOV = Date.UTC(2026, 10, 1) / 1000;
const JAN = Date.UTC(2027, 0, 1) / 1000;
const MAR = Date.UTC(2026, 2, 1) / 1000;

function goal(versions: BudgetGoal['versions']): BudgetGoal {
  return {
    id: 'goal',
    categoryId: 'cat_groceries',
    timeframe: 'monthly',
    amount: '8000.00',
    currency: 'CZK',
    createdAt: 0,
    updatedAt: 0,
    effectiveMonth: MAR,
    versions,
  };
}

describe('limitDefaults', () => {
  it('opens an addition empty, from this month, without an end', () => {
    expect(limitDefaults(null, undefined, OCT)).toEqual({
      amountText: '',
      from: '2026-10',
      until: '',
    });
  });

  it('starts an addition at the month the report shows', () => {
    expect(limitDefaults(null, NOV, OCT).from).toBe('2026-11');
  });

  it('opens an edit on the limit in effect this month, an open end staying open', () => {
    const limit = goal([{ effectiveMonth: MAR, amount: '8000.00', currency: 'CZK' }]);
    expect(limitDefaults(limit, undefined, OCT)).toEqual({
      amountText: '8000',
      from: '2026-10',
      until: '',
    });
  });

  it('keeps the end of the edited interval', () => {
    const limit = goal([
      { effectiveMonth: MAR, amount: '8000.00', currency: 'CZK' },
      { effectiveMonth: JAN, amount: '9000.50', currency: 'CZK' },
    ]);
    expect(limitDefaults(limit, undefined, OCT)).toEqual({
      amountText: '8000',
      from: '2026-10',
      until: '2026-12',
    });
    // The interval the report's month falls in, in its own amount
    expect(limitDefaults(limit, JAN, OCT)).toEqual({
      amountText: '9000.5',
      from: '2027-01',
      until: '',
    });
  });
});
