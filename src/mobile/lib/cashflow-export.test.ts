import { describe, expect, it } from 'vitest';
import type { CashflowActuals, CashflowGroup } from '@shared/schema';
import { CASHFLOW_EXPORT_FILE, cashflowCsv } from './cashflow-export';

const group = (patch: Partial<CashflowGroup>): CashflowGroup => ({
  key: 'cat_food',
  name: 'Food',
  flowKind: 'expense',
  amount: '4200.50',
  count: 12,
  ...patch,
});

function actuals(patch: Partial<CashflowActuals> = {}): CashflowActuals {
  return {
    reportCurrency: 'CZK',
    periodStart: 1_759_276_800,
    periodEnd: 1_761_955_199,
    quality: {
      missingFxCount: 0,
      missingFxCurrencies: [],
      estimatedFxCount: 0,
      observedCompletedMonths: [],
      partialMonthCount: 0,
      unobservedMonthCount: 0,
      invalidAmountCount: 0,
    },
    months: [
      { month: 1_756_684_800, income: '52000.00', expenses: '31000.25' },
      { month: 1_759_276_800, income: '54000.00', expenses: '33100.75' },
    ],
    totalIncome: '54000.00',
    totalExpenses: '33100.75',
    netBalance: '20899.25',
    bankCashMovement: '20899.25',
    averageIncome: null,
    averageExpenses: null,
    averageCompletedMonths: [],
    expenseCategories: [group({})],
    incomeCategories: [
      group({
        key: 'cat_salary',
        name: 'Salary',
        flowKind: 'income',
        amount: '54000.00',
        count: 1,
      }),
    ],
    transfers: [
      group({
        key: 'cat_savings',
        name: 'Savings',
        flowKind: 'transfer',
        amount: '-5000',
        count: 2,
      }),
    ],
    accountCount: 2,
    transactionCount: 15,
    ...patch,
  };
}

const month = (seconds: number) => `m${seconds}`;
const label = (g: CashflowGroup) => `label:${g.key}`;

describe('cashflowCsv', () => {
  it('writes the desktop CSV and counts months plus one group per section', () => {
    const { csv, count } = cashflowCsv(actuals(), month, label);
    expect(count).toBe(5);
    expect(csv.split('\n')).toEqual([
      'month;income_czk;expenses_czk',
      'm1756684800;52000.00;31000.25',
      'm1759276800;54000.00;33100.75',
      '',
      'expense_category;amount_czk;transactions',
      'label:cat_food;4200.50;12',
      '',
      'income_category;amount_czk;transactions',
      'label:cat_salary;54000.00;1',
      '',
      'transfer;amount_czk;transactions',
      'label:cat_savings;-5000;2',
    ]);
  });

  it('names the report currency in the headers', () => {
    const { csv } = cashflowCsv(actuals({ reportCurrency: 'EUR' }), month, label);
    expect(csv).toContain('month;income_eur;expenses_eur');
    expect(csv).toContain('transfer;amount_eur;transactions');
  });

  it('counts every group of a section, not only the first', () => {
    const { count } = cashflowCsv(
      actuals({
        expenseCategories: [group({ key: 'a' }), group({ key: 'b' }), group({ key: 'c' })],
        transfers: [],
      }),
      month,
      label
    );
    // 2 months + 3 expense groups + 1 income group + 0 transfers
    expect(count).toBe(6);
  });

  it('counts nothing for a period without a transaction, however many zero months it carries', () => {
    const empty = actuals({
      transactionCount: 0,
      totalIncome: '0',
      totalExpenses: '0',
      netBalance: '0',
      expenseCategories: [],
      incomeCategories: [],
      transfers: [],
    });
    expect(empty.months).toHaveLength(2);
    expect(cashflowCsv(empty, month, label).count).toBe(0);
  });

  it('quotes a label that holds the delimiter', () => {
    const { csv } = cashflowCsv(
      actuals({ expenseCategories: [group({ key: 'x' })] }),
      month,
      () => 'Food; and "drink"'
    );
    expect(csv).toContain('"Food; and ""drink""";4200.50;12');
  });
});

describe('CASHFLOW_EXPORT_FILE', () => {
  it('is the fixed name Rust allow-lists', () => {
    expect(CASHFLOW_EXPORT_FILE).toBe('moony-cashflow.csv');
  });
});
