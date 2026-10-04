import type { Loan } from '@shared/schema';
import { convertToCzK, type CurrencyCode } from '@shared/currencies';
import {
  dayFloor,
  dueDay,
  loanTermsFromWire,
  loanValuationMode,
  paymentBelowInterest,
  type LoanTerms,
  type LoanValuationMode,
} from '@shared/calculations/loan-amortization';
import { loanTrajectory, type LoanTrajectory } from '@/utils/loan-trajectory';

const DAY = 86_400;

/** Derived numbers of one loan for the list page (CZK totals, native per-loan figures). */
export interface LoanRow {
  loan: Loan;
  terms: LoanTerms;
  trajectory: LoanTrajectory;
  mode: LoanValuationMode;
  /** The payment does not cover the interest: the balance does not fall. */
  belowInterest: boolean;
  /** Multiplier from the loan's currency to CZK. */
  rate: number;
  /** Backend-valued balance as of today, native and CZK. */
  outstanding: number;
  outstandingCzk: number;
  principal: number;
  principalCzk: number;
  payment: number;
  paymentCzk: number;
  /** 1 − outstanding / principal, clamped to 0–1. */
  repaidShare: number;
  /** Paid off, or past the contractual end date. */
  matured: boolean;
  payoffDay: number | null;
  /** Interest in the payments due within the next 12 months, CZK. */
  interestNext12Czk: number;
  /** Rate validity date still ahead of today. */
  fixationEnd: number | null;
}

export function loanRow(loan: Loan, today: number): LoanRow {
  const day = dayFloor(today);
  const terms = loanTermsFromWire(loan);
  const trajectory = loanTrajectory(terms, day);
  const currency = (loan.currency || 'CZK') as CurrencyCode;
  const rate = convertToCzK(1, currency);
  const outstanding = Math.max(0, Number(loan.outstandingBalance) || 0);
  const principal = Number(loan.principal) || 0;
  const payment = Number(loan.monthlyPayment) || 0;
  const horizon = dueDay(day, 12);
  const interestNext12 = trajectory.upcoming
    .filter((r) => r.dueDay <= horizon)
    .reduce((s, r) => s + r.interest, 0);
  const matured = outstanding <= 0 || (loan.endDate !== null && dayFloor(loan.endDate) < day);
  return {
    loan,
    terms,
    trajectory,
    mode: loanValuationMode(terms),
    belowInterest: paymentBelowInterest(terms),
    rate,
    outstanding,
    outstandingCzk: outstanding * rate,
    principal,
    principalCzk: principal * rate,
    payment,
    paymentCzk: payment * rate,
    repaidShare: principal > 0 ? Math.min(1, Math.max(0, 1 - outstanding / principal)) : 0,
    matured,
    payoffDay: trajectory.payoffDay,
    interestNext12Czk: interestNext12 * rate,
    fixationEnd:
      loan.interestRateValidityDate !== null && dayFloor(loan.interestRateValidityDate) > day
        ? dayFloor(loan.interestRateValidityDate)
        : null,
  };
}

export interface LoanMetrics {
  count: number;
  activeCount: number;
  totalOutstanding: number;
  totalPrincipal: number;
  totalMonthlyPayment: number;
  /** Weighted by the outstanding balance in CZK; 0 without any balance. */
  averageInterestRate: number;
  interestNext12: number;
  /** The active loan that pays off first, with the monthly payments left after it. */
  nextPayoff: { row: LoanRow; day: number; paymentsAfter: number } | null;
  /** The nearest fixation end ahead of today. */
  nextFixation: { row: LoanRow; day: number } | null;
  /** A rate validity date already behind us (the loan needs a new rate). */
  expiredFixation: { row: LoanRow; day: number } | null;
  /** Day all active loans are paid off; null when any never is. */
  debtFreeDay: number | null;
}

/** Totals over the rows; matured loans count only in `count` and the principal. */
export function loanMetrics(rows: readonly LoanRow[], today: number): LoanMetrics {
  const day = dayFloor(today);
  const active = rows.filter((r) => !r.matured);
  const totalOutstanding = active.reduce((s, r) => s + r.outstandingCzk, 0);
  const weighted = active.reduce(
    (s, r) => s + r.outstandingCzk * (Number(r.loan.interestRate) || 0),
    0
  );
  const withPayoff = active.filter((r) => r.payoffDay !== null && r.payoffDay > day);
  const first = withPayoff.sort((a, b) => a.payoffDay! - b.payoffDay!)[0];
  const fixations = active
    .filter((r) => r.fixationEnd !== null)
    .sort((a, b) => a.fixationEnd! - b.fixationEnd!);
  const expired = active
    .filter(
      (r) =>
        r.loan.interestRateValidityDate !== null && dayFloor(r.loan.interestRateValidityDate) <= day
    )
    .sort((a, b) => b.loan.interestRateValidityDate! - a.loan.interestRateValidityDate!)[0];
  const everyPaysOff = active.length > 0 && active.every((r) => r.payoffDay !== null);
  return {
    count: rows.length,
    activeCount: active.length,
    totalOutstanding,
    totalPrincipal: rows.reduce((s, r) => s + r.principalCzk, 0),
    totalMonthlyPayment: active.reduce((s, r) => s + r.paymentCzk, 0),
    averageInterestRate: totalOutstanding > 0 ? weighted / totalOutstanding : 0,
    interestNext12: active.reduce((s, r) => s + r.interestNext12Czk, 0),
    nextPayoff: first
      ? {
          row: first,
          day: first.payoffDay!,
          paymentsAfter: active.reduce((s, r) => s + (r === first ? 0 : r.paymentCzk), 0),
        }
      : null,
    nextFixation: fixations[0] ? { row: fixations[0], day: fixations[0].fixationEnd! } : null,
    expiredFixation: expired
      ? { row: expired, day: dayFloor(expired.loan.interestRateValidityDate!) }
      : null,
    debtFreeDay: everyPaysOff ? Math.max(...active.map((r) => r.payoffDay!)) : null,
  };
}

/** Months from `from` to `to` as the user would count them (whole months, UTC). */
export function monthsAhead(from: number, to: number): number {
  const a = new Date(dayFloor(from) * 1000);
  const b = new Date(dayFloor(to) * 1000);
  let months = (b.getUTCFullYear() - a.getUTCFullYear()) * 12 + (b.getUTCMonth() - a.getUTCMonth());
  if (b.getUTCDate() < a.getUTCDate()) months -= 1;
  return Math.max(0, months);
}

export { DAY as LOAN_DAY };
