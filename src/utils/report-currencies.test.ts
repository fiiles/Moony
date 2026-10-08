import { describe, expect, it } from 'vitest';
import { buildCashflowCsv, summarizeBudgetStatus } from './report-currencies';

describe('report currency boundaries', () => {
  it('exports target decimal TEXT without converting or forcing two decimals', () => {
    const csv = buildCashflowCsv(
      {
        reportCurrency: 'KWD',
        months: [
          {
            month: 1,
            income: '100.123',
            expenses: '0.001',
            nonconsumptionFlows: [],
            bankCashMovement: '100.122',
          },
        ],
        expenseCategories: [
          { key: 'food', name: 'Food', amount: '99.999', previousAmount: '1.234', count: 2 },
        ],
        incomeSources: [],
      },
      () => 'Jan 2026',
      (group) => group.name,
      (group) => group.name
    );
    expect(csv).toContain('income_kwd;expenses_kwd');
    expect(csv).toContain('Jan 2026;100.123;0.001');
    expect(csv).toContain('Food;99.999;1.234;2');
  });

  it('keeps native EUR and JPY export values as supplied by their reports', () => {
    const empty = { months: [], expenseCategories: [], incomeSources: [] };
    expect(
      buildCashflowCsv(
        {
          ...empty,
          reportCurrency: 'EUR',
          months: [
            {
              month: 1,
              income: '100.00',
              expenses: '0.00',
              nonconsumptionFlows: [],
              bankCashMovement: '100.00',
            },
          ],
        },
        String,
        () => '',
        () => ''
      )
    ).toContain('1;100.00;0.00');
    expect(
      buildCashflowCsv(
        {
          ...empty,
          reportCurrency: 'JPY',
          months: [
            {
              month: 1,
              income: '100',
              expenses: '1',
              nonconsumptionFlows: [],
              bankCashMovement: '99',
            },
          ],
        },
        String,
        () => '',
        () => ''
      )
    ).toContain('1;100;1');
  });

  it('counts mixed-currency budget states without summing native amounts', () => {
    expect(
      summarizeBudgetStatus(
        [
          {
            budgetCurrency: 'EUR',
            budgetSpent: '90',
            budgetLimitNative: '100',
            budgetPercentage: 90,
          },
          {
            budgetCurrency: 'USD',
            budgetSpent: '110',
            budgetLimitNative: '100',
            budgetPercentage: 110,
          },
          {
            budgetCurrency: 'KWD',
            budgetSpent: null,
            budgetLimitNative: '50',
            budgetPercentage: null,
          },
          {
            budgetCurrency: 'JPY',
            budgetSpent: '20',
            budgetLimitNative: '100',
            budgetPercentage: 20,
          },
        ],
        '345.67'
      )
    ).toEqual({ targetExpenses: '345.67', onTrack: 1, near: 1, over: 1, unavailable: 1 });
  });
});
