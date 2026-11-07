import { describe, expect, it } from 'vitest';
import type { Loan } from '@shared/schema';
import { loanRow } from '@/utils/loans';
import { upcomingLoanPayments } from './upcoming-premiums';

const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;
const TODAY = day(2026, 11, 7);

function loan(partial: Partial<Loan> = {}): Loan {
  return {
    id: 'l1',
    name: 'Hypotéka',
    principal: '3000000',
    currency: 'CZK',
    interestRate: '4.2',
    interestRateValidityDate: null,
    monthlyPayment: '24800',
    startDate: day(2022, 3, 15),
    endDate: day(2047, 3, 15),
    balanceAnchorAmount: null,
    balanceAnchorDate: null,
    outstandingBalance: '2700000.00',
    balanceMode: 'estimated',
    events: [],
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}
const rowOf = (l: Loan) => loanRow(l, TODAY, (amount) => amount);

describe('upcomingLoanPayments', () => {
  it("lists the next 90 days' payments on the loan's day of the month", () => {
    const payments = upcomingLoanPayments([rowOf(loan())], TODAY);
    expect(payments.map((p) => p.day)).toEqual([
      day(2026, 11, 15),
      day(2026, 12, 15),
      day(2027, 1, 15),
    ]);
    expect(payments.every((p) => p.amount === 24800 && p.currency === 'CZK')).toBe(true);
    expect(payments[0]).toMatchObject({ loanId: 'l1', name: 'Hypotéka' });
  });

  it('includes a payment due today and keeps a shorter window', () => {
    const payments = upcomingLoanPayments([rowOf(loan())], day(2026, 11, 15), 20);
    expect(payments.map((p) => p.day)).toEqual([day(2026, 11, 15)]);
  });

  it('merges the loans by day', () => {
    const payments = upcomingLoanPayments(
      [
        rowOf(loan({ id: 'b', name: 'Byt' })),
        rowOf(loan({ id: 'a', name: 'Auto', startDate: day(2024, 1, 10), monthlyPayment: '5000' })),
      ],
      TODAY,
      30
    );
    expect(payments.map((p) => [p.name, p.day])).toEqual([
      ['Auto', day(2026, 11, 10)],
      ['Byt', day(2026, 11, 15)],
    ]);
  });

  it('has nothing for a paid-off loan or one without a payment', () => {
    const paidOff = loan({ endDate: day(2026, 1, 15) });
    const noPayment = loan({ monthlyPayment: '0' });
    expect(upcomingLoanPayments([rowOf(paidOff), rowOf(noPayment)], TODAY)).toEqual([]);
  });
});
