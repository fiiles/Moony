import { describe, expect, it } from 'vitest';
import type { Loan } from '@shared/schema';
import { linkedLoanInput, rentalResult, rentalSeries, type RentalInput } from './rental';

const PROTO: RentalInput = {
  price: 5_400_000,
  loan: 3_000_000,
  loanYears: 30,
  rate: 4.1,
  monthlyRent: 21_500,
  monthlyCosts: 3_000,
  vacancyMonths: 0.5,
  rentGrowth: 3,
  costsGrowth: 3,
  priceGrowth: 3,
  years: 15,
  oneTimeCosts: 250_000,
};

describe('rentalResult', () => {
  it('reproduces the prototype figures', () => {
    const r = rentalResult(PROTO);
    expect(r.equity).toBe(2_400_000);
    expect(r.invested).toBe(2_650_000);
    expect(r.ltv).toBeCloseTo(55.6, 1);
    expect(r.monthlyPayment).toBeCloseTo(14_496, 0);
    expect(r.grossYield).toBeCloseTo(4.78, 2);
    expect(r.years).toHaveLength(15);
    expect(r.years[14].value).toBeCloseTo(5_400_000 * 1.03 ** 15, 0);
    expect(r.annualReturn).toBeGreaterThan(0.04);
    expect(r.annualReturn).toBeLessThan(0.1);
  });

  it('shifts only the price growth for the band', () => {
    const mid = rentalSeries(PROTO);
    const hi = rentalSeries(PROTO, 1);
    expect(hi[0].rent).toBe(mid[0].rent);
    expect(hi[0].balance).toBe(mid[0].balance);
    expect(hi[14].value).toBeGreaterThan(mid[14].value);
    expect(hi[14].equity - mid[14].equity).toBeCloseTo(hi[14].value - mid[14].value, 6);
  });

  it('finds the milestone years and copes without a loan', () => {
    const r = rentalResult({ ...PROTO, monthlyRent: 10_000 });
    expect(r.positiveCashflowYear).toBeNull();
    const cash = rentalResult({ ...PROTO, loan: 0 });
    expect(cash.monthlyPayment).toBe(0);
    expect(cash.halfLoanYear).toBeNull();
    expect(cash.positiveCashflowYear).toBe(1);
    expect(cash.years[0].payments).toBe(0);
  });
});

const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;

function loan(overrides: Partial<Loan>): Loan {
  return {
    id: 'l1',
    name: 'Mortgage',
    principal: '3000000',
    currency: 'CZK',
    interestRate: '4.1',
    interestRateValidityDate: null,
    monthlyPayment: '14496',
    startDate: day(2020, 3, 15),
    endDate: day(2050, 3, 15),
    balanceAnchorAmount: null,
    balanceAnchorDate: null,
    outstandingBalance: '2600000',
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

/** 1 EUR = 25 CZK, CZK is the display currency. */
const toCzk = (amount: number, currency: string) => (currency === 'EUR' ? amount * 25 : amount);

describe('linkedLoanInput', () => {
  it('is an empty loan without linked loans', () => {
    expect(linkedLoanInput([], toCzk)).toEqual({ loan: 0, loanYears: null, rate: null });
  });

  it('takes the principal, the contractual term and the rate of one loan', () => {
    const r = linkedLoanInput([loan({})], toCzk);
    expect(r.loan).toBe(3_000_000);
    expect(r.loanYears).toBeCloseTo(30, 1);
    expect(r.rate).toBe(4.1);
  });

  it('derives the term from the payment when the loan has no end date', () => {
    const r = linkedLoanInput([loan({ endDate: null })], toCzk);
    expect(r.loanYears).toBeCloseTo(30, 1);
    const zeroRate = linkedLoanInput(
      [loan({ endDate: null, interestRate: '0', principal: '120000', monthlyPayment: '1000' })],
      toCzk
    );
    expect(zeroRate.loanYears).toBe(10);
  });

  it('leaves the term unknown when the payment does not cover the interest', () => {
    expect(linkedLoanInput([loan({ endDate: null, monthlyPayment: '0' })], toCzk).loanYears).toBe(
      null
    );
    expect(
      linkedLoanInput([loan({ endDate: null, monthlyPayment: '10000' })], toCzk).loanYears
    ).toBe(null);
  });

  it('sums several loans in the display currency and weights rate and term by principal', () => {
    const r = linkedLoanInput(
      [
        loan({ principal: '2000000', interestRate: '4', endDate: day(2040, 3, 15) }),
        loan({
          id: 'l2',
          principal: '40000',
          currency: 'EUR',
          interestRate: '7',
          endDate: day(2030, 3, 15),
        }),
      ],
      toCzk
    );
    expect(r.loan).toBe(3_000_000);
    expect(r.rate).toBeCloseTo(5, 6);
    expect(r.loanYears).toBeCloseTo(50 / 3, 1);
  });
});
