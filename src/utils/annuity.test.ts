import { describe, it, expect } from 'vitest';
import {
  calculateAnnuityPayment,
  generateAmortizationSchedule,
  getPeriodsPerYear,
  yearsToTotalPeriods,
  monthsToTotalPeriods,
} from './annuity';

describe('getPeriodsPerYear', () => {
  it('returns 12 for monthly', () => {
    expect(getPeriodsPerYear('monthly')).toBe(12);
  });
  it('returns 4 for quarterly', () => {
    expect(getPeriodsPerYear('quarterly')).toBe(4);
  });
  it('returns 2 for semiAnnually', () => {
    expect(getPeriodsPerYear('semiAnnually')).toBe(2);
  });
  it('returns 1 for annually', () => {
    expect(getPeriodsPerYear('annually')).toBe(1);
  });
});

describe('calculateAnnuityPayment', () => {
  it('computes standard monthly payment at 5% for 240 months on 3,000,000', () => {
    // PMT = 3000000 * (0.004167 * (1.004167^240)) / ((1.004167^240) - 1) ≈ 19799
    const payment = calculateAnnuityPayment(3_000_000, 5, 240, 12);
    expect(payment).toBeCloseTo(19799, -1); // within 10
  });

  it('returns principal / periods when rate is 0', () => {
    const payment = calculateAnnuityPayment(120_000, 0, 120, 12);
    expect(payment).toBeCloseTo(1000, 1);
  });

  it('returns 0 when principal is 0', () => {
    expect(calculateAnnuityPayment(0, 5, 120, 12)).toBe(0);
  });

  it('returns 0 when totalPeriods is 0', () => {
    expect(calculateAnnuityPayment(100_000, 5, 0, 12)).toBe(0);
  });

  it('returns full principal for 1 period at 0% rate', () => {
    expect(calculateAnnuityPayment(50_000, 0, 1, 12)).toBeCloseTo(50_000, 1);
  });
});

describe('generateAmortizationSchedule', () => {
  it('returns empty schedule when principal is 0', () => {
    const result = generateAmortizationSchedule(0, 5, 120, 12);
    expect(result.periodicPayment).toBe(0);
    expect(result.schedule).toHaveLength(0);
  });

  it('schedule has correct number of rows', () => {
    const result = generateAmortizationSchedule(500_000, 4, 60, 12);
    expect(result.schedule).toHaveLength(60);
  });

  it('last row has remaining balance near 0', () => {
    const result = generateAmortizationSchedule(500_000, 4, 60, 12);
    const last = result.schedule[result.schedule.length - 1];
    expect(last.remainingBalance).toBeCloseTo(0, 0);
  });

  it('totalPayments equals periodicPayment * periods', () => {
    const result = generateAmortizationSchedule(200_000, 3.5, 24, 12);
    expect(result.totalPayments).toBeCloseTo(result.periodicPayment * 24, 2);
  });

  it('principal + interest of first period = payment amount', () => {
    const result = generateAmortizationSchedule(1_000_000, 6, 120, 12);
    const first = result.schedule[0];
    expect(first.principalPayment + first.interestPayment).toBeCloseTo(first.payment, 5);
  });
});

describe('yearsToTotalPeriods', () => {
  it('converts 30 years monthly to 360 periods', () => {
    expect(yearsToTotalPeriods(30, 12)).toBe(360);
  });
  it('converts 5 years quarterly to 20 periods', () => {
    expect(yearsToTotalPeriods(5, 4)).toBe(20);
  });
});

describe('monthsToTotalPeriods', () => {
  it('converts 36 months monthly to 36 periods', () => {
    expect(monthsToTotalPeriods(36, 12)).toBe(36);
  });
  it('converts 12 months quarterly to 4 periods', () => {
    expect(monthsToTotalPeriods(12, 4)).toBe(4);
  });
});

describe('extra yearly payment and yearly aggregation', () => {
  it('an extra yearly payment shortens the loan and lowers the interest', async () => {
    const { generateAmortizationSchedule, aggregateByYear } = await import('./annuity');
    const base = generateAmortizationSchedule(3_500_000, 5.1, 25 * 12, 12);
    const extra = generateAmortizationSchedule(3_500_000, 5.1, 25 * 12, 12, 50_000);
    expect(extra.schedule.length).toBeLessThan(base.schedule.length);
    expect(extra.totalInterest).toBeLessThan(base.totalInterest);
    expect(extra.schedule[extra.schedule.length - 1].remainingBalance).toBe(0);
    // December carries the extra 50 000 on top of the annuity
    expect(extra.schedule[11].payment).toBeCloseTo(extra.periodicPayment + 50_000, 6);
    // Totals are consistent with the rows
    const paid = extra.schedule.reduce((s, r) => s + r.payment, 0);
    expect(extra.totalPayments).toBeCloseTo(paid, 6);

    const years = aggregateByYear(base.schedule);
    expect(years).toHaveLength(25);
    expect(years[0].payment).toBeCloseTo(base.periodicPayment * 12, 6);
    expect(years[24].balance).toBeCloseTo(0, 6);
    expect(years[0].interest).toBeGreaterThan(years[0].principal);
  });
});
