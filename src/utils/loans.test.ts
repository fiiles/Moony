import { describe, expect, it } from 'vitest';
import type { Loan } from '@shared/schema';
import { loanMetrics, loanRow, monthsAhead } from './loans';

const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;
const TODAY = day(2026, 10, 3);
/** 1 EUR = 25 CZK; no rate for KWD. */
const convert = (amount: number, currency: string) =>
  currency === 'KWD' ? null : currency === 'EUR' ? amount * 25 : amount;

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
    balanceMode: 'estimated',
    events: [],
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}

describe('loanRow', () => {
  it('derives the repaid share, payoff and fixation from the wire loan', () => {
    const row = loanRow(
      loan({ interestRateValidityDate: day(2028, 3, 1), monthlyPayment: '10000' }),
      TODAY,
      convert
    );
    expect(row.converted?.outstanding).toBe(870_000);
    expect(row.repaidShare).toBeCloseTo(0.275, 3);
    expect(row.matured).toBe(false);
    expect(row.fixationEnd).toBe(day(2028, 3, 1));
    // 33 payments made (Feb 2024 .. Oct 2026): 1 200 000 − 330 000 = 870 000, 87 to go
    expect(row.trajectory.balanceToday).toBe(870_000);
    expect(row.payoffDay).toBe(day(2034, 1, 1));
    expect(row.converted?.interestNext12).toBe(0);
  });

  it('values a loan with a manual balance from its ledger: it holds, the plan continues from it', () => {
    const recordDay = day(2026, 6, 1);
    const row = loanRow(
      loan({
        balanceAnchorAmount: '500000',
        balanceAnchorDate: recordDay,
        outstandingBalance: '500000.00',
        balanceMode: 'manual',
        events: [
          {
            id: 'e1',
            loanId: 'l',
            kind: 'balance_check',
            eventDay: recordDay,
            amount: '500000',
            rate: null,
            monthlyPayment: null,
            previousRate: null,
            previousMonthlyPayment: null,
            note: null,
            createdAt: 1,
          },
        ],
      }),
      TODAY,
      convert
    );
    expect(row.mode).toBe('manual');
    expect(row.trajectory.balanceToday).toBe(500_000);
    // The estimated path ahead of the first record is the old line: 28 payments (Feb 2024 .. May 2026)
    expect(row.trajectory.past.find((p) => p.t === recordDay - 86_400)?.value).toBe(920_000);
    // 50 payments of 10 000 from the 1st after today: Nov 2026 .. Dec 2030
    expect(row.payoffDay).toBe(day(2030, 12, 1));
    expect(row.plan.anchorAmount).toBe(500_000);
    expect(row.matured).toBe(false);
  });

  it('does not call a loan that has not started paid off', () => {
    const row = loanRow(
      loan({ startDate: day(2026, 12, 1), outstandingBalance: '0.00' }),
      TODAY,
      convert
    );
    expect(row.matured).toBe(false);
    expect(row.repaidShare).toBe(0);
    expect(row.trajectory.balanceToday).toBe(0);
  });

  it('marks paid-off and ended loans as matured and converts foreign currency', () => {
    expect(loanRow(loan({ outstandingBalance: '0.00' }), TODAY, convert).matured).toBe(true);
    expect(loanRow(loan({ endDate: day(2026, 1, 1) }), TODAY, convert).matured).toBe(true);
    const eur = loanRow(loan({ currency: 'EUR', outstandingBalance: '100' }), TODAY, convert);
    expect(eur.converted?.outstanding).toBe(2_500);
  });
});

describe('loanMetrics', () => {
  it('weights the rate by balance and finds the next payoff and fixation', () => {
    const big = loanRow(
      loan({ id: 'a', interestRate: '4', interestRateValidityDate: day(2028, 3, 1) }),
      TODAY,
      convert
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
      TODAY,
      convert
    );
    const old = loanRow(loan({ id: 'c', outstandingBalance: '0.00' }), TODAY, convert);
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

  it('excludes unconvertible loans from the totals and reports them', () => {
    const czk = loanRow(loan({ id: 'a', interestRate: '4' }), TODAY, convert);
    const eur = loanRow(
      loan({ id: 'b', currency: 'EUR', outstandingBalance: '100', interestRate: '2' }),
      TODAY,
      convert
    );
    const kwd = loanRow(
      loan({ id: 'c', currency: 'KWD', outstandingBalance: '7000', interestRate: '9' }),
      TODAY,
      convert
    );
    expect(kwd.converted).toBeNull();
    expect(kwd.outstanding).toBe(7000);
    const m = loanMetrics([czk, eur, kwd], TODAY);
    expect(m.totalOutstanding).toBe(870_000 + 2_500);
    expect(m.totalMonthlyPayment).toBe(10_000 + 250_000);
    expect(m.averageInterestRate).toBeCloseTo((870_000 * 4 + 2_500 * 2) / 872_500, 6);
    expect(m.activeCount).toBe(3);
    expect(m.missing).toEqual(['KWD']);
  });

  it('has no debt-free day when a loan never pays off', () => {
    const stuck = loanRow(loan({ monthlyPayment: '0' }), TODAY, convert);
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
