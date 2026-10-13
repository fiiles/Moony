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
 *
 * Ledger (`loanBalanceAt`, `planFrom`): the balance on a day is a walk over the loan's
 * events. A manual balance record holds exactly until the user changes it (nothing is
 * estimated over it); estimated segments amortize monthly from their anchor day. See
 * `docs/specs/2026-10-13-history-from-dated-facts-design.md`, B2 and B3.
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

/** The stored kind of a loan event; `balance_check` is the manual balance record. */
export type LedgerEventKind = 'extra_payment' | 'rate_change' | 'balance_check';

/** One loan event, already parsed. Money is in the loan currency. */
export interface LedgerEvent {
  /** UTC day of the event. */
  day: number;
  /** Orders events that share a day (unix seconds). */
  createdAt: number;
  kind: LedgerEventKind;
  /** Extra payment: the amount paid. Manual record: the balance. */
  amount?: number | null;
  /** Rate change: the new annual rate in percent (null reads as 0, like an empty rate). */
  rate?: number | null;
  /** Extra payment / rate change: the new regular payment when given. */
  monthlyPayment?: number | null;
  /** Rate change: the rate in force before it (null on legacy rows). */
  previousRate?: number | null;
  /**
   * Rate change, or an extra payment that sets a new payment: the payment in force before it
   * (null on legacy rows and on events that leave it as is).
   */
  previousMonthlyPayment?: number | null;
}

/**
 * A loan and its events. `terms` holds the loan's current rate and payment, the principal,
 * the start day and the end day; its anchor fields are ignored (the events are the source
 * of every manual balance).
 */
export interface LoanLedger {
  terms: LoanTerms;
  events: LedgerEvent[];
}

/** How the balance of a day came about. */
export type LedgerMode = 'estimated' | 'manual';

/** The walk's state on a day. */
export interface LedgerState {
  /** Balance on the day, `>= 0`; 0 before the start day. */
  balance: number;
  mode: LedgerMode;
  /**
   * Day whose day-of-month the monthly payments fall on: the start day, or the day of the
   * last estimated extra payment / rate change. A manual record leaves it unchanged.
   */
  segmentAnchorDay: number;
  /** Balance on `segmentAnchorDay`; equals `balance` in `manual` mode. */
  segmentAnchorAmount: number;
  /** Annual rate in force on the day, percent. */
  annualRatePct: number;
  /** Regular payment in force on the day. */
  monthlyPayment: number;
}

/** Events by (day, created time), the order the walk applies them in. */
function orderedEvents(ledger: LoanLedger): LedgerEvent[] {
  return [...ledger.events].sort(
    (a, b) => dayFloor(a.day) - dayFloor(b.day) || a.createdAt - b.createdAt
  );
}

/**
 * Rate and payment before any event: each is the previous value recorded by the EARLIEST event
 * that carries one (any kind: a rate change records both, an extra payment that lowers the
 * payment records the payment), else the loan's current terms (legacy rows).
 */
function initialTerms(
  terms: LoanTerms,
  events: LedgerEvent[]
): { annualRatePct: number; monthlyPayment: number } {
  const rate = events.find(
    (event) => event.previousRate !== null && event.previousRate !== undefined
  );
  const payment = events.find(
    (event) => event.previousMonthlyPayment !== null && event.previousMonthlyPayment !== undefined
  );
  return {
    annualRatePct: rate?.previousRate ?? terms.annualRatePct,
    monthlyPayment: payment?.previousMonthlyPayment ?? terms.monthlyPayment,
  };
}

/** Balance of the current estimated segment after every payment due on or before `toDay`. */
function estimatedBalanceThrough(state: LedgerState, toDay: number): number {
  return elapsedTo(
    {
      principal: state.segmentAnchorAmount,
      annualRatePct: state.annualRatePct,
      monthlyPayment: state.monthlyPayment,
      startDay: state.segmentAnchorDay,
    },
    toDay
  ).balance;
}

function applyEvent(state: LedgerState, event: LedgerEvent): void {
  const estimated = state.mode === 'estimated';
  if (event.kind === 'balance_check') {
    state.balance = Math.max(0, event.amount ?? 0);
    state.mode = 'manual';
  } else if (event.kind === 'extra_payment') {
    state.balance = Math.max(0, state.balance - (event.amount ?? 0));
    if (event.monthlyPayment !== null && event.monthlyPayment !== undefined) {
      state.monthlyPayment = event.monthlyPayment;
    }
  } else {
    state.annualRatePct = event.rate ?? 0;
    if (event.monthlyPayment !== null && event.monthlyPayment !== undefined) {
      state.monthlyPayment = event.monthlyPayment;
    }
  }
  if (estimated && event.kind !== 'balance_check') {
    // The event day starts a new estimated segment.
    state.segmentAnchorDay = dayFloor(event.day);
  }
  state.segmentAnchorAmount = state.balance;
}

/**
 * Balance of a loan on `day`: the ledger walk (spec B2).
 *
 * Before the start day the balance is 0. Otherwise the walk starts as `estimated` from
 * (principal, start day), with the terms in force before the first event (`initialTerms`), and
 * applies the events dated on or before `day` in (day, created time) order:
 * - estimated segments amortize monthly between events, on the segment anchor's day of month;
 * - a manual record (`balance_check`) sets the balance and switches to `manual`: nothing is
 *   amortized after it;
 * - an extra payment subtracts its amount (a new payment applies when given); estimated: the
 *   event day becomes the segment anchor; manual: stays manual;
 * - a rate change swaps the terms; estimated: the event day becomes the segment anchor;
 *   manual: the balance is unchanged.
 */
export function loanBalanceAt(ledger: LoanLedger, day: number): LedgerState {
  const target = dayFloor(day);
  const start = dayFloor(ledger.terms.startDay);
  const events = orderedEvents(ledger);
  const principal = Math.max(0, ledger.terms.principal);
  const state: LedgerState = {
    balance: principal,
    mode: 'estimated',
    segmentAnchorDay: start,
    segmentAnchorAmount: principal,
    ...initialTerms(ledger.terms, events),
  };
  if (target < start) {
    state.balance = 0;
    return state;
  }
  for (const event of events) {
    if (dayFloor(event.day) > target) break;
    if (state.mode === 'estimated') {
      state.balance = estimatedBalanceThrough(state, dayFloor(event.day));
    }
    applyEvent(state, event);
  }
  if (state.mode === 'estimated') state.balance = estimatedBalanceThrough(state, target);
  state.balance = Math.max(0, state.balance);
  return state;
}

/**
 * Terms whose anchor starts the schedule ahead, so `amortizationSchedule` and `elapsedTo`
 * run on them unchanged (spec B3). The terms are the ones in force on `today`:
 * - `estimated`: the schedule continues from the segment anchor, as before;
 * - `manual`: it starts from today's balance. Payments stay on the segment anchor's day of
 *   month, so the anchor day is the last payment day on or before `today` of that cycle, and
 *   the first planned payment is the next cycle day strictly after `today` (the balance
 *   already includes today's). A cycle day above 28 that the anchor month clamps (e.g. the
 *   31st in a 30-day month) moves the later due days to the clamped day.
 */
export function planFrom(ledger: LoanLedger, today: number): LoanTerms {
  const state = loanBalanceAt(ledger, today);
  const anchorAmount = state.mode === 'estimated' ? state.segmentAnchorAmount : state.balance;
  const anchorDay =
    state.mode === 'estimated'
      ? state.segmentAnchorDay
      : dueDay(state.segmentAnchorDay, paymentsDueThrough(state.segmentAnchorDay, today));
  return {
    principal: ledger.terms.principal,
    annualRatePct: state.annualRatePct,
    monthlyPayment: state.monthlyPayment,
    startDay: ledger.terms.startDay,
    anchorAmount,
    anchorDay,
    endDay: ledger.terms.endDay ?? null,
  };
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
