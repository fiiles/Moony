/**
 * Annuity Calculator Utility
 *
 * Provides functions for calculating annuity loan payments and generating
 * amortization schedules. Can be reused throughout the app. The per-period
 * step (interest, principal, balance) is the one the loans use
 * (`shared/calculations/loan-amortization.ts`), so the calculator and a loan
 * with the same terms cannot diverge.
 */

import { amortizationStep } from '@shared/calculations/loan-amortization';

export type PaymentPeriodicity = 'monthly' | 'quarterly' | 'semiAnnually' | 'annually';

export interface AmortizationRow {
  periodNumber: number;
  year: number;
  month: number;
  payment: number;
  principalPayment: number;
  interestPayment: number;
  remainingBalance: number;
}

export interface AnnuityCalculationResult {
  periodicPayment: number;
  totalPayments: number;
  totalInterest: number;
  schedule: AmortizationRow[];
}

/** One year of the schedule, summed (prototype `annuity-calculator.html`). */
export interface AmortizationYear {
  /** 1-based year of the loan. */
  year: number;
  payment: number;
  principal: number;
  interest: number;
  /** Balance after the year's last payment. */
  balance: number;
}

/**
 * Get the number of payment periods per year based on periodicity
 */
export function getPeriodsPerYear(periodicity: PaymentPeriodicity): number {
  switch (periodicity) {
    case 'monthly':
      return 12;
    case 'quarterly':
      return 4;
    case 'semiAnnually':
      return 2;
    case 'annually':
      return 1;
  }
}

/**
 * Calculate the periodic payment for an annuity loan
 *
 * @param principal - The loan amount
 * @param annualInterestRate - Annual interest rate as percentage (e.g., 5.5 for 5.5%)
 * @param totalPeriods - Total number of payment periods
 * @param periodsPerYear - Number of payment periods per year (12 for monthly, 4 for quarterly, etc.)
 * @returns The periodic payment amount
 */
export function calculateAnnuityPayment(
  principal: number,
  annualInterestRate: number,
  totalPeriods: number,
  periodsPerYear: number
): number {
  if (principal <= 0 || totalPeriods <= 0) {
    return 0;
  }

  // Convert annual rate to periodic rate
  const periodicRate = annualInterestRate / 100 / periodsPerYear;

  // Handle zero interest rate case
  if (periodicRate === 0) {
    return principal / totalPeriods;
  }

  // Standard annuity formula: PMT = P * (r(1+r)^n) / ((1+r)^n - 1)
  const compoundFactor = Math.pow(1 + periodicRate, totalPeriods);
  const payment = (principal * (periodicRate * compoundFactor)) / (compoundFactor - 1);

  return payment;
}

/**
 * Generate a complete amortization schedule for an annuity loan
 *
 * @param principal - The loan amount
 * @param annualInterestRate - Annual interest rate as percentage (e.g., 5.5 for 5.5%)
 * @param totalPeriods - Total number of payment periods
 * @param periodsPerYear - Number of payment periods per year
 * @returns Full calculation result including payment amount and amortization schedule
 */
export function generateAmortizationSchedule(
  principal: number,
  annualInterestRate: number,
  totalPeriods: number,
  periodsPerYear: number,
  /**
   * Optional extra payment made once a year after the year's last regular
   * payment (no penalty). It goes straight to the principal, so the loan
   * ends earlier and the schedule gets shorter than `totalPeriods`.
   */
  extraYearlyPayment = 0
): AnnuityCalculationResult {
  const periodicPayment = calculateAnnuityPayment(
    principal,
    annualInterestRate,
    totalPeriods,
    periodsPerYear
  );

  if (periodicPayment === 0) {
    return {
      periodicPayment: 0,
      totalPayments: 0,
      totalInterest: 0,
      schedule: [],
    };
  }

  const periodicRate = annualInterestRate / 100 / periodsPerYear;
  const schedule: AmortizationRow[] = [];
  let remainingBalance = principal;
  let totalInterest = 0;
  let totalPayments = 0;

  // Calculate month increment based on periodicity
  const monthsPerPeriod = 12 / periodsPerYear;

  for (let period = 1; period <= totalPeriods && remainingBalance > 0.005; period++) {
    const step = amortizationStep(remainingBalance, periodicRate, periodicPayment);
    const interestPayment = step.interest;
    let principalPayment = step.principalPart;
    remainingBalance = step.balanceAfter;
    let payment = step.payment;
    if (extraYearlyPayment > 0 && period % periodsPerYear === 0 && remainingBalance > 0) {
      const extra = Math.min(remainingBalance, extraYearlyPayment);
      remainingBalance -= extra;
      principalPayment += extra;
      payment += extra;
    }
    totalInterest += interestPayment;
    totalPayments += payment;

    // Calculate year and month for this period
    const totalMonths = period * monthsPerPeriod;
    const year = Math.ceil(totalMonths / 12);
    const month = ((totalMonths - 1) % 12) + 1;

    schedule.push({
      periodNumber: period,
      year,
      month,
      payment,
      principalPayment,
      interestPayment,
      remainingBalance,
    });
  }

  return {
    periodicPayment,
    // Without extras every payment is the annuity, so this equals payment × periods
    totalPayments: extraYearlyPayment > 0 ? totalPayments : periodicPayment * schedule.length,
    totalInterest,
    schedule,
  };
}

/** Sum the schedule per loan year, keeping the balance after each year. */
export function aggregateByYear(schedule: AmortizationRow[]): AmortizationYear[] {
  const years: AmortizationYear[] = [];
  for (const row of schedule) {
    let y = years[years.length - 1];
    if (!y || y.year !== row.year) {
      y = { year: row.year, payment: 0, principal: 0, interest: 0, balance: row.remainingBalance };
      years.push(y);
    }
    y.payment += row.payment;
    y.principal += row.principalPayment;
    y.interest += row.interestPayment;
    y.balance = row.remainingBalance;
  }
  return years;
}

/**
 * Convert period in years to total number of payment periods
 */
export function yearsToTotalPeriods(years: number, periodsPerYear: number): number {
  return years * periodsPerYear;
}

/**
 * Convert period in months to total number of payment periods
 */
export function monthsToTotalPeriods(months: number, periodsPerYear: number): number {
  return Math.ceil(months / (12 / periodsPerYear));
}
