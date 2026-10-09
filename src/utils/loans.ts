import type { Loan } from '@shared/schema';
import { MissingTracker, type ConvertFn } from '@shared/fx';
import {
  dayFloor,
  dueDay,
  loanTermsFromWire,
  loanValuationMode,
  paymentBelowInterest,
  type LoanTerms,
  type LoanValuationMode,
} from '@shared/calculations/loan-amortization';
import { recordCurrency } from '@shared/currencies';
import { loanTrajectory, type LoanTrajectory } from '@/utils/loan-trajectory';

const DAY = 86_400;

/** A loan's money in the main currency at the current quotes. */
export interface LoanConverted {
  outstanding: number;
  principal: number;
  payment: number;
  /** Interest in the payments due within the next 12 months. */
  interestNext12: number;
}

/** Derived numbers of one loan for the list page (native figures, main-currency totals). */
export interface LoanRow {
  loan: Loan;
  terms: LoanTerms;
  trajectory: LoanTrajectory;
  mode: LoanValuationMode;
  /** The payment does not cover the interest: the balance does not fall. */
  belowInterest: boolean;
  /** The loan's currency (legacy rows without one are CZK). */
  currency: string;
  /** Backend-valued balance as of today, native. */
  outstanding: number;
  principal: number;
  payment: number;
  /** Interest due within the next 12 months, native. */
  interestNext12: number;
  /** The money above in the main currency; null when the currency has no rate. */
  converted: LoanConverted | null;
  /** 1 − outstanding / principal, clamped to 0–1. */
  repaidShare: number;
  /** Paid off, or past the contractual end date. */
  matured: boolean;
  payoffDay: number | null;
  /** Rate validity date still ahead of today. */
  fixationEnd: number | null;
}

export function loanRow(loan: Loan, today: number, convert: ConvertFn): LoanRow {
  const day = dayFloor(today);
  const terms = loanTermsFromWire(loan);
  const trajectory = loanTrajectory(terms, day);
  const currency = recordCurrency(loan.currency);
  // One factor per loan: every amount of the row converts at the same current quote
  const rate = convert(1, currency);
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
    currency,
    outstanding,
    principal,
    payment,
    interestNext12,
    converted:
      rate === null
        ? null
        : {
            outstanding: outstanding * rate,
            principal: principal * rate,
            payment: payment * rate,
            interestNext12: interestNext12 * rate,
          },
    repaidShare: principal > 0 ? Math.min(1, Math.max(0, 1 - outstanding / principal)) : 0,
    matured,
    payoffDay: trajectory.payoffDay,
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
  /** Weighted by the outstanding balance in the main currency; 0 without any balance. */
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
  /** Currencies of the loans left out of the totals for want of a rate. */
  missing: string[];
}

/** Sum of one converted field over the rows that have a rate. */
function sumConverted(rows: readonly LoanRow[], field: keyof LoanConverted): number {
  return rows.reduce((s, r) => s + (r.converted?.[field] ?? 0), 0);
}

/**
 * Totals in the main currency over the rows with a rate (the others are reported in
 * `missing`); matured loans count only in `count` and the principal.
 */
export function loanMetrics(rows: readonly LoanRow[], today: number): LoanMetrics {
  const day = dayFloor(today);
  const active = rows.filter((r) => !r.matured);
  const tracker = new MissingTracker();
  for (const r of rows) tracker.track(r.currency, r.converted === null ? null : 0);
  const totalOutstanding = sumConverted(active, 'outstanding');
  const weighted = active.reduce(
    (s, r) => s + (r.converted?.outstanding ?? 0) * (Number(r.loan.interestRate) || 0),
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
    totalPrincipal: sumConverted(rows, 'principal'),
    totalMonthlyPayment: sumConverted(active, 'payment'),
    averageInterestRate: totalOutstanding > 0 ? weighted / totalOutstanding : 0,
    interestNext12: sumConverted(active, 'interestNext12'),
    nextPayoff: first
      ? {
          row: first,
          day: first.payoffDay!,
          paymentsAfter: active.reduce(
            (s, r) => s + (r === first ? 0 : (r.converted?.payment ?? 0)),
            0
          ),
        }
      : null,
    nextFixation: fixations[0] ? { row: fixations[0], day: fixations[0].fixationEnd! } : null,
    expiredFixation: expired
      ? { row: expired, day: dayFloor(expired.loan.interestRateValidityDate!) }
      : null,
    debtFreeDay: everyPaysOff ? Math.max(...active.map((r) => r.payoffDay!)) : null,
    missing: tracker.missing,
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
