import { describe, expect, it } from 'vitest';
import { rentalResult, rentalSeries, type RentalInput } from './rental';

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
