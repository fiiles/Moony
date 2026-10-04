import { describe, expect, it } from 'vitest';
import type { Loan } from '@shared/schema';
import { loanMetrics, loanRow, monthsAhead } from './loans';

const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;
const TODAY = day(2026, 10, 3);

function loan(partial: Partial<Loan>): Loan {
  return {
    id: 'l',
    name: 'Mortgage',
    principal: '1200000',
    currency: 'CZK',
    interestRate: '0',
    interestRateValidityDate: null,
    monthlyPayment: '10000',
    startDate: day(2024, 1, 1),
    endDate: null,
    balanceAnchorAmount: null,
    balanceAnchorDate: null,
    outstandingBalance: '870000.00',
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}

describe('loanRow', () => {
  it('derives the repaid share, payoff and fixation from the wire loan', () => {
    const row = loanRow(
      loan({ interestRateValidityDate: day(2028, 3, 1), monthlyPayment: '10000' }),
      TODAY
    );
    expect(row.outstandingCzk).toBe(870_000);
    expect(row.repaidShare).toBeCloseTo(0.275, 3);
    expect(row.matured).toBe(false);
    expect(row.fixationEnd).toBe(day(2028, 3, 1));
    // 33 payments made (Feb 2024 .. Oct 2026): 1 200 000 − 330 000 = 870 000, 87 to go
    expect(row.trajectory.balanceToday).toBe(870_000);
    expect(row.payoffDay).toBe(day(2034, 1, 1));
    expect(row.interestNext12Czk).toBe(0);
  });

  it('marks paid-off and ended loans as matured and converts foreign currency', () => {
    expect(loanRow(loan({ outstandingBalance: '0.00' }), TODAY).matured).toBe(true);
    expect(loanRow(loan({ endDate: day(2026, 1, 1) }), TODAY).matured).toBe(true);
    const eur = loanRow(loan({ currency: 'EUR', outstandingBalance: '100' }), TODAY);
    expect(eur.outstandingCzk).toBeGreaterThan(100);
  });
});

describe('loanMetrics', () => {
  it('weights the rate by balance and finds the next payoff and fixation', () => {
    const big = loanRow(
      loan({ id: 'a', interestRate: '4', interestRateValidityDate: day(2028, 3, 1) }),
      TODAY
    );
    const small = loanRow(
      loan({
        id: 'b',
        name: 'Car',
        principal: '300000',
        outstandingBalance: '30000.00',
        monthlyPayment: '10000',
        interestRate: '8',
        startDate: day(2024, 7, 1),
      }),
      TODAY
    );
    const old = loanRow(loan({ id: 'c', outstandingBalance: '0.00' }), TODAY);
    const m = loanMetrics([big, small, old], TODAY);
    expect(m.count).toBe(3);
    expect(m.activeCount).toBe(2);
    expect(m.totalOutstanding).toBe(900_000);
    expect(m.averageInterestRate).toBeCloseTo((870_000 * 4 + 30_000 * 8) / 900_000, 6);
    expect(m.nextPayoff?.row.loan.id).toBe('b');
    expect(m.nextPayoff?.paymentsAfter).toBe(10_000);
    expect(m.nextFixation?.day).toBe(day(2028, 3, 1));
    expect(m.debtFreeDay).toBe(big.payoffDay);
  });

  it('has no debt-free day when a loan never pays off', () => {
    const stuck = loanRow(loan({ monthlyPayment: '0' }), TODAY);
    expect(loanMetrics([stuck], TODAY).debtFreeDay).toBeNull();
    expect(loanMetrics([stuck], TODAY).nextPayoff).toBeNull();
  });
});

describe('monthsAhead', () => {
  it('counts whole months only', () => {
    expect(monthsAhead(day(2026, 10, 3), day(2028, 3, 1))).toBe(16);
    expect(monthsAhead(day(2026, 10, 3), day(2028, 3, 3))).toBe(17);
    expect(monthsAhead(day(2026, 10, 3), day(2026, 9, 1))).toBe(0);
  });
});
