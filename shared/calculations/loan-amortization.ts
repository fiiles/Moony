/**
 * Loan amortization maths.
 *
 * Mirror of `src-tauri/src/services/loan_amortization.rs`: the Rust side is
 * authoritative (net worth, schedule command); this module backs the loan form's
 * live estimate, the valuation tooltips and the annuity calculator. Both are pinned
 * to the cent by `loan-amortization.fixture.json`.
 *
 * Days are UTC-midnight unix seconds (ADR 0008). Rules:
 * - Monthly rate `i = annual% / 100 / 12`. Payment `k` falls due on the anchor's
 *   day-of-month, `k` calendar months after the anchor, clamped to the last day of a
 *   short month; a payment due on the valuation day counts as made.
 * - The anchor is the (amount, day) amortization runs from: the manual balance when
 *   both are set, else (principal, start day).
 * - No monthly payment: the balance stays at the anchor.
 */

export const SECONDS_PER_DAY = 86_400;

/** Longest schedule returned to the UI (50 years of monthly payments). */
export const MAX_SCHEDULE_ROWS = 600;

/** Valuation never loops past 100 years of payments. */
const MAX_VALUATION_MONTHS = 1_200;

export interface LoanTerms {
  /** Original amount; the anchor amount when no manual balance is set. */
  principal: number;
  /** Annual interest rate in percent (0 for an empty rate). */
  annualRatePct: number;
  /** Monthly payment; `<= 0` means unknown: no amortization. */
  monthlyPayment: number;
  /** UTC day the loan started; the anchor day when no manual balance is set. */
  startDay: number;
  anchorAmount?: number | null;
  anchorDay?: number | null;
  /** Schedule rows stop at the last payment due on or before this day. */
  endDay?: number | null;
}

export interface AmortizationStep {
  interest: number;
  principalPart: number;
  /** The payment actually made: smaller than the regular one on the last row. */
  payment: number;
  balanceAfter: number;
}

/**
 * The single amortization formula, shared by the loans and the annuity calculator:
 * `interest = b·i`, `principal = payment − interest`, `b' = b − principal`; the final
 * payment clears the balance and never overshoots it. A payment below the interest
 * yields a negative principal part, i.e. the balance grows.
 */
export function amortizationStep(
  balance: number,
  monthlyRate: number,
  payment: number
): AmortizationStep {
  const interest = balance * monthlyRate;
  const principal = payment - interest;
  if (principal >= balance) {
    return { interest, principalPart: balance, payment: balance + interest, balanceAfter: 0 };
  }
  return { interest, principalPart: principal, payment, balanceAfter: balance - principal };
}

/** Start of the UTC day containing `ts` (unix seconds). */
export function dayFloor(ts: number): number {
  return Math.floor(ts / SECONDS_PER_DAY) * SECONDS_PER_DAY;
}

/** Today's UTC day. */
export function todayUtcDay(now: Date = new Date()): number {
  return dayFloor(now.getTime() / 1000);
}

function daysInMonth(year: number, month0: number): number {
  return new Date(Date.UTC(year, month0 + 1, 0)).getUTCDate();
}

/** The (amount, day) amortization starts from. */
export function loanAnchor(terms: LoanTerms): { amount: number; day: number; isManual: boolean } {
  if (
    terms.anchorAmount !== null &&
    terms.anchorAmount !== undefined &&
    terms.anchorDay !== null &&
    terms.anchorDay !== undefined
  ) {
    return {
      amount: Math.max(0, terms.anchorAmount),
      day: dayFloor(terms.anchorDay),
      isManual: true,
    };
  }
  return { amount: Math.max(0, terms.principal), day: dayFloor(terms.startDay), isManual: false };
}

function monthlyRate(terms: LoanTerms): number {
  return terms.annualRatePct / 100 / 12;
}

/** Day payment `k` falls due (k = 0 is the anchor day itself). */
export function dueDay(anchorDay: number, k: number): number {
  const start = new Date(anchorDay * 1000);
  const total = start.getUTCMonth() + k;
  const year = start.getUTCFullYear() + Math.floor(total / 12);
  const month0 = ((total % 12) + 12) % 12;
  const dayOfMonth = Math.min(start.getUTCDate(), daysInMonth(year, month0));
  return Date.UTC(year, month0, dayOfMonth) / 1000;
}

/** Number of payments due on or before `day` (0 before the first one). */
export function paymentsDueThrough(anchorDay: number, day: number): number {
  const anchor = dayFloor(anchorDay);
  const target = dayFloor(day);
  if (target < anchor) return 0;
  const a = new Date(anchor * 1000);
  const d = new Date(target * 1000);
  const months = Math.max(
    0,
    (d.getUTCFullYear() - a.getUTCFullYear()) * 12 + d.getUTCMonth() - a.getUTCMonth()
  );
  return months > 0 && dueDay(anchor, months) > target ? months - 1 : months;
}

export interface ElapsedLoan {
  balance: number;
  interestPaid: number;
  paymentsMade: number;
}

/** Balance and cumulative figures after every payment due on or before `day`. */
export function elapsedTo(terms: LoanTerms, day: number): ElapsedLoan {
  const anchor = loanAnchor(terms);
  const elapsed: ElapsedLoan = { balance: anchor.amount, interestPaid: 0, paymentsMade: 0 };
  if (terms.monthlyPayment <= 0 || anchor.amount <= 0 || dayFloor(day) < anchor.day) {
    return elapsed;
  }
  const rate = monthlyRate(terms);
  const due = Math.min(paymentsDueThrough(anchor.day, day), MAX_VALUATION_MONTHS);
  for (let n = 0; n < due && elapsed.balance > 0; n++) {
    const step = amortizationStep(elapsed.balance, rate, terms.monthlyPayment);
    elapsed.interestPaid += step.interest;
    elapsed.paymentsMade += 1;
    elapsed.balance = step.balanceAfter;
  }
  return elapsed;
}

/**
 * Outstanding balance as of `day`: the anchor amount before the anchor day, else the
 * anchor after every payment due on or before `day`.
 */
export function outstandingBalanceAt(terms: LoanTerms, day: number): number {
  return elapsedTo(terms, day).balance;
}

/** True when the payment does not cover the first month's interest (balance stays or grows). */
export function paymentBelowInterest(terms: LoanTerms): boolean {
  const anchor = loanAnchor(terms);
  return (
    terms.monthlyPayment > 0 &&
    anchor.amount > 0 &&
    terms.monthlyPayment <= anchor.amount * monthlyRate(terms)
  );
}

/** How the balance of a loan is valued, for tooltips. */
export type LoanValuationMode = 'automatic' | 'manual' | 'static';

export function loanValuationMode(terms: LoanTerms): LoanValuationMode {
  if (terms.monthlyPayment <= 0) return 'static';
  return loanAnchor(terms).isManual ? 'manual' : 'automatic';
}

export interface AmortizationScheduleRow {
  /** Day the payment falls due; the interest covers the month ending then. */
  dueDay: number;
  payment: number;
  interest: number;
  principalPart: number;
  balanceAfter: number;
}

/** Why a schedule stopped. */
export type AmortizationScheduleEnd = 'paid' | 'endDate' | 'capped' | 'noPayment';

export interface AmortizationSchedule {
  rows: AmortizationScheduleRow[];
  end: AmortizationScheduleEnd;
}

/** Payments from the anchor until the balance is 0, the last due on or before `endDay`, or `maxRows`. */
export function amortizationSchedule(
  terms: LoanTerms,
  maxRows: number = MAX_SCHEDULE_ROWS
): AmortizationSchedule {
  const anchor = loanAnchor(terms);
  const rows: AmortizationScheduleRow[] = [];
  if (terms.monthlyPayment <= 0) return { rows, end: 'noPayment' };
  if (anchor.amount <= 0) return { rows, end: 'paid' };
  const rate = monthlyRate(terms);
  const endDay =
    terms.endDay === null || terms.endDay === undefined ? null : dayFloor(terms.endDay);
  let balance = anchor.amount;
  for (let k = 1; ; k++) {
    const due = dueDay(anchor.day, k);
    if (endDay !== null && due > endDay) return { rows, end: 'endDate' };
    if (rows.length >= maxRows) return { rows, end: 'capped' };
    const step = amortizationStep(balance, rate, terms.monthlyPayment);
    balance = step.balanceAfter;
    rows.push({
      dueDay: due,
      payment: step.payment,
      interest: step.interest,
      principalPart: step.principalPart,
      balanceAfter: step.balanceAfter,
    });
    if (balance <= 0) return { rows, end: 'paid' };
  }
}

/** The loan fields the maths needs, as they come over the wire. */
export interface LoanWireTerms {
  principal: string;
  interestRate: string;
  monthlyPayment: string;
  startDate: number;
  endDate: number | null;
  balanceAnchorAmount: string | null;
  balanceAnchorDate: number | null;
}

/** Empty or unparseable text counts as 0, like the backend's empty-rate rule. */
function parseAmount(text: string | null | undefined): number {
  if (text === null || text === undefined || text.trim() === '') return 0;
  const value = Number(text);
  return Number.isFinite(value) ? value : 0;
}

export function loanTermsFromWire(loan: LoanWireTerms): LoanTerms {
  const hasAnchor =
    loan.balanceAnchorAmount !== null &&
    loan.balanceAnchorAmount.trim() !== '' &&
    loan.balanceAnchorDate !== null;
  return {
    principal: parseAmount(loan.principal),
    annualRatePct: parseAmount(loan.interestRate),
    monthlyPayment: parseAmount(loan.monthlyPayment),
    startDay: dayFloor(loan.startDate),
    anchorAmount: hasAnchor ? parseAmount(loan.balanceAnchorAmount) : null,
    anchorDay: hasAnchor ? dayFloor(loan.balanceAnchorDate as number) : null,
    endDay: loan.endDate === null ? null : dayFloor(loan.endDate),
  };
}
