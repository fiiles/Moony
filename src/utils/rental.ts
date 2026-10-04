/**
 * Real estate investment calculator (design system §7 Calculator, prototype
 * `rental-calculator.html`): cashflow, equity and the sale after N years,
 * with the price growth shifted for the scenario band. Pure functions.
 */
import type { Loan } from '@shared/schema';
import { calculateAnnuityPayment } from '@/utils/annuity';

const SECONDS_PER_YEAR = 365.25 * 86_400;

export interface RentalInput {
  price: number;
  loan: number;
  loanYears: number;
  /** Annual interest rate in percent. */
  rate: number;
  monthlyRent: number;
  monthlyCosts: number;
  /** Vacant months per year. */
  vacancyMonths: number;
  /** Annual growth rates in percent. */
  rentGrowth: number;
  costsGrowth: number;
  priceGrowth: number;
  /** Investment horizon in years (≥ 1). */
  years: number;
  /** One-time costs at purchase (fees, renovation), part of the invested capital. */
  oneTimeCosts: number;
}

export interface RentalYear {
  /** 1-based year of the investment. */
  year: number;
  rent: number;
  costs: number;
  payments: number;
  cashflow: number;
  /** Loan balance at the end of the year. */
  balance: number;
  /** Property value at the end of the year. */
  value: number;
  /** Cumulative cashflow. */
  cumulative: number;
  /** Value − balance + cumulative cashflow. */
  equity: number;
}

export interface RentalResult {
  monthlyPayment: number;
  equity: number;
  invested: number;
  ltv: number;
  /** Net monthly cashflow in the first year (vacancy and costs deducted). */
  netMonthly: number;
  /** Gross monthly cashflow in the first year (rent − payment). */
  grossMonthly: number;
  /** Annual rent / purchase price, in percent. */
  grossYield: number;
  years: RentalYear[];
  /** (final equity − invested) / invested */
  totalReturn: number;
  /** Annualised total return (CAGR), as a fraction. */
  annualReturn: number;
  /** First year with a positive yearly cashflow (1-based), or null. */
  positiveCashflowYear: number | null;
  /** First year in which the balance is at most half the loan (1-based), or null. */
  halfLoanYear: number | null;
}

/** The yearly series with the price growth shifted by `priceShift` percentage points. */
export function rentalSeries(input: RentalInput, priceShift = 0): RentalYear[] {
  const years = Math.max(1, Math.round(input.years));
  const monthlyRate = input.rate / 100 / 12;
  const payment =
    input.loan > 0 && input.loanYears > 0
      ? calculateAnnuityPayment(input.loan, input.rate, input.loanYears * 12, 12)
      : 0;
  const occupancy = 1 - Math.min(12, Math.max(0, input.vacancyMonths)) / 12;
  const growth = (g: number) => 1 + g / 100;
  let balance = input.loan;
  let cumulative = 0;
  const rows: RentalYear[] = [];
  for (let y = 1; y <= years; y++) {
    const rent = input.monthlyRent * growth(input.rentGrowth) ** (y - 1) * 12 * occupancy;
    const costs = input.monthlyCosts * growth(input.costsGrowth) ** (y - 1) * 12;
    let payments = 0;
    for (let m = 0; m < 12 && balance > 0.005; m++) {
      const interest = balance * monthlyRate;
      const principal = Math.min(balance, payment - interest);
      balance -= principal;
      payments += principal + interest;
    }
    balance = Math.max(0, balance);
    const cashflow = rent - costs - payments;
    cumulative += cashflow;
    const value = input.price * growth(input.priceGrowth + priceShift) ** y;
    rows.push({
      year: y,
      rent,
      costs,
      payments,
      cashflow,
      balance,
      value,
      cumulative,
      equity: value - balance + cumulative,
    });
  }
  return rows;
}

export function rentalResult(input: RentalInput): RentalResult {
  const years = rentalSeries(input);
  const monthlyPayment =
    input.loan > 0 && input.loanYears > 0
      ? calculateAnnuityPayment(input.loan, input.rate, input.loanYears * 12, 12)
      : 0;
  const equity = input.price - input.loan;
  const invested = equity + input.oneTimeCosts;
  const occupancy = 1 - Math.min(12, Math.max(0, input.vacancyMonths)) / 12;
  const netMonthly = input.monthlyRent * occupancy - input.monthlyCosts - monthlyPayment;
  const last = years[years.length - 1];
  const totalReturn = invested > 0 ? (last.equity - invested) / invested : 0;
  const annualReturn =
    invested > 0 && last.equity > 0 ? (last.equity / invested) ** (1 / years.length) - 1 : 0;
  const positive = years.findIndex((y) => y.cashflow > 0);
  const half = years.findIndex((y) => y.balance <= input.loan / 2);
  return {
    monthlyPayment,
    equity,
    invested,
    ltv: input.price > 0 ? (input.loan / input.price) * 100 : 0,
    netMonthly,
    grossMonthly: input.monthlyRent - monthlyPayment,
    grossYield: input.price > 0 ? ((input.monthlyRent * 12) / input.price) * 100 : 0,
    years,
    totalReturn,
    annualReturn,
    positiveCashflowYear: positive >= 0 ? positive + 1 : null,
    halfLoanYear: input.loan > 0 && half >= 0 ? half + 1 : null,
  };
}

/** Loan fields of the calculator, prefilled from a property's linked loans. */
export interface LinkedLoanInput {
  /** Sum of the original principals in the display currency (0 without loans). */
  loan: number;
  /** Principal-weighted term in years; null when no loan has a known term. */
  loanYears: number | null;
  /** Principal-weighted annual rate in percent; null without loans. */
  rate: number | null;
}

/**
 * Contractual term of a loan in years: start to end date, else the number of
 * payments that repay the principal (null when the payment does not cover the interest).
 */
function loanTermYears(loan: Loan): number | null {
  if (loan.endDate !== null && loan.endDate > loan.startDate) {
    return (loan.endDate - loan.startDate) / SECONDS_PER_YEAR;
  }
  const principal = Number(loan.principal) || 0;
  const payment = Number(loan.monthlyPayment) || 0;
  const monthlyRate = (Number(loan.interestRate) || 0) / 100 / 12;
  if (principal <= 0 || payment <= 0) return null;
  if (monthlyRate === 0) return principal / payment / 12;
  if (payment <= principal * monthlyRate) return null;
  return -Math.log(1 - (monthlyRate * principal) / payment) / Math.log(1 + monthlyRate) / 12;
}

/**
 * The calculator's loan from the loans linked to a property: the principals
 * summed in the display currency (the purchase price is the purchase-time
 * figure too), the rate and the term weighted by principal.
 */
export function linkedLoanInput(
  loans: Loan[],
  toDisplay: (amount: number, currency: string) => number
): LinkedLoanInput {
  let loan = 0;
  let rateSum = 0;
  let termSum = 0;
  let termWeight = 0;
  for (const l of loans) {
    const principal = toDisplay(Number(l.principal) || 0, l.currency || 'CZK');
    loan += principal;
    rateSum += principal * (Number(l.interestRate) || 0);
    const term = loanTermYears(l);
    if (term !== null) {
      termSum += principal * term;
      termWeight += principal;
    }
  }
  return {
    loan,
    loanYears: termWeight > 0 ? termSum / termWeight : null,
    rate: loan > 0 ? rateSum / loan : null,
  };
}
