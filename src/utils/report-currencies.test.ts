import { describe, expect, it } from 'vitest';
import { createFormatters } from '@/lib/format';
import { buildCashflowCsv, formatReportMoney, summarizeBudgetStatus } from './report-currencies';

describe('report currency boundaries', () => {
  it('reconciles displayed CZK contributions with the total and the export', () => {
    // Two CZK 100.40 rows: the cash style (0 decimals) would show 100 + 100 = 201.
    const report = {
      reportCurrency: 'CZK',
      months: [
        {
          month: 1,
          income: '0.00',
          expenses: '200.80',
          nonconsumptionFlows: [],
          bankCashMovement: '-200.80',
        },
      ],
      expenseCategories: [
        { key: 'food', name: 'Food', amount: '100.40', previousAmount: '0.00', count: 1 },
        { key: 'fuel', name: 'Fuel', amount: '100.40', previousAmount: '0.00', count: 1 },
      ],
      incomeSources: [],
    };
    const fmt = createFormatters('en', 'CZK');
    const shown = (text: string) => Number(text.replace(/[^\d.-]/g, ''));
    const plain = (text: string) => text.replace(/\s/g, ' ');
    const rows = report.expenseCategories.map((g) =>
      plain(formatReportMoney(fmt, Number(g.amount), report.reportCurrency))
    );
    const total = plain(
      formatReportMoney(fmt, Number(report.months[0].expenses), report.reportCurrency)
    );
    expect(rows).toEqual(['Kč 100.40', 'Kč 100.40']);
    expect(total).toBe('Kč 200.80');
    expect(rows.map(shown).reduce((a, b) => a + b, 0)).toBeCloseTo(shown(total), 10);
    const csv = buildCashflowCsv(
      report,
      () => 'Jan 2026',
      (g) => g.name,
      (g) => g.name
    );
    expect(csv).toContain('Jan 2026;0.00;200.80');
    expect(csv).toContain('Food;100.40;0.00;1');
    // Cash style elsewhere in the app is unchanged.
    expect(plain(fmt.money(200.8, 'CZK'))).toBe('Kč 201');
  });

  it('uses ISO precision beyond the picker and for signed native details', () => {
    const fmt = createFormatters('en', 'CZK');
    expect(formatReportMoney(fmt, 1.2345, 'JOD')).toMatch(/1\.235/);
    expect(formatReportMoney(fmt, 1500.4, 'UGX')).toMatch(/1,500$/);
    expect(formatReportMoney(fmt, 0.123, 'KWD')).toMatch(/0\.123/);
    expect(formatReportMoney(createFormatters('en', 'JPY'), 1234, 'JPY')).toBe('¥1,234');
    const signed = formatReportMoney(fmt, -100.4, 'CZK', { signed: true });
    expect(signed.replace(/\s/g, ' ')).toBe('\u2212 Kč 100.40');
  });

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
