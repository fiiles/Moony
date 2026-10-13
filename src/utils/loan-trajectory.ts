import {
  amortizationSchedule,
  amortizationStep,
  dayFloor,
  dueDay,
  loanAnchor,
  loanBalanceAt,
  planFrom,
  type AmortizationSchedule,
  type AmortizationScheduleEnd,
  type AmortizationScheduleRow,
  type LoanLedger,
  type LoanTerms,
} from '@shared/calculations/loan-amortization';

/** One balance reading of a loan (unix seconds, native currency). */
export interface TrajectoryPoint {
  t: number;
  value: number;
}

const DAY = 86_400;
/** The past path never walks more than a century of payments in one segment. */
const MAX_PAST_MONTHS = 1_200;
/** Combined debt series are thinned to about this many points for the chart. */
const MAX_SERIES_POINTS = 480;

export interface LoanTrajectory {
  /** Balance at the start, after every payment up to `today`, at every step, and today. */
  past: TrajectoryPoint[];
  /** Balance after every scheduled payment due after `today`. */
  future: TrajectoryPoint[];
  /** The plan ahead: the schedule from the walk's plan terms (shared maths, same as the backend). */
  schedule: AmortizationSchedule;
  /** Rows of the schedule due after `today`. */
  upcoming: AmortizationScheduleRow[];
  end: AmortizationScheduleEnd;
  /** The balance on `today` from the ledger walk; 0 for a loan that has not started. */
  balanceToday: number;
  /** Interest in the payments made so far (estimated segments of the past path). */
  interestPaid: number;
  /** First day the balance reaches 0 (past or future); null when it never does. */
  payoffDay: number | null;
  /** First day the balance is at or below half the principal; null when it never is. */
  halfDay: number | null;
}

/** The schedule ahead of `day` for plan terms: every row, those after `day`, and their balances. */
function planAhead(plan: LoanTerms, day: number) {
  const schedule = amortizationSchedule(plan);
  const upcoming = schedule.rows.filter((row) => row.dueDay > day);
  const future = upcoming.map((row) => ({ t: row.dueDay, value: row.balanceAfter }));
  return { schedule, upcoming, future };
}

/**
 * Day a plan pays the loan off: the anchor day for an empty anchor, else the first scheduled
 * payment that clears the balance; null when it never does.
 */
export function planPayoffDay(plan: LoanTerms, today: number): number | null {
  const anchor = loanAnchor(plan);
  if (anchor.amount <= 0) return anchor.day;
  return planAhead(plan, dayFloor(today)).future.find((p) => p.value <= 0)?.t ?? null;
}

/**
 * The balance path up to `today` from the ledger walk (spec B2, D5). It follows the estimated
 * path (one reading per payment due) until the first step, and every event that moves the
 * balance (a manual record, an extra payment) is a vertical step on its day, with a reading
 * the day before so it is not drawn as a ramp from the previous payment. After a manual record
 * nothing is amortized, so the line holds flat until the next event. A loan that starts after
 * `today` has its start reading only.
 */
function pastPath(
  ledger: LoanLedger,
  today: number
): { points: TrajectoryPoint[]; interestPaid: number } {
  const start = dayFloor(ledger.terms.startDay);
  const principal = Math.max(0, ledger.terms.principal);
  const points: TrajectoryPoint[] = [{ t: start, value: principal }];
  if (start > today) return { points, interestPaid: 0 };

  const push = (t: number, value: number) => {
    const last = points[points.length - 1];
    if (last.t === t) last.value = value;
    else points.push({ t, value });
  };

  // The days the walk changes course on: the start and every event day up to today.
  const bounds = [
    start,
    ...[...new Set(ledger.events.map((event) => dayFloor(event.day)))]
      .filter((d) => d > start && d <= today)
      .sort((a, b) => a - b),
  ];

  let interestPaid = 0;
  for (let i = 0; i < bounds.length; i++) {
    const from = bounds[i];
    const next = bounds[i + 1];
    const state = loanBalanceAt(ledger, from);
    if (i === 0) {
      push(from, state.balance);
    } else {
      const before = loanBalanceAt(ledger, from - DAY).balance;
      if (Math.abs(state.balance - before) > 1e-9) {
        // A step: hold the old balance until the day before, then jump
        if (from - DAY > points[points.length - 1].t) push(from - DAY, before);
        push(from, state.balance);
      }
    }
    if (state.mode !== 'estimated' || state.monthlyPayment <= 0) continue;
    // Payments due after this day up to the next boundary (the boundary day's own payment
    // belongs to this segment, but its reading is the boundary's, after the event)
    const rate = state.annualRatePct / 100 / 12;
    let balance = state.balance;
    for (let k = 1; balance > 0 && k <= MAX_PAST_MONTHS; k++) {
      const due = dueDay(state.segmentAnchorDay, k);
      if (due <= from) continue;
      if (due > (next ?? today)) break;
      const step = amortizationStep(balance, rate, state.monthlyPayment);
      interestPaid += step.interest;
      balance = step.balanceAfter;
      if (next === undefined || due < next) push(due, balance);
    }
  }
  const last = points[points.length - 1];
  if (last.t < today) push(today, loanBalanceAt(ledger, today).balance);
  return { points, interestPaid };
}

/**
 * The balance path of one loan: solid past from the ledger walk (the estimated path before the
 * first manual balance, every manual record a step on its day), dashed future from the plan
 * ahead (`planFrom`).
 */
export function loanTrajectory(ledger: LoanLedger, today: number): LoanTrajectory {
  const day = dayFloor(today);
  const { points: past, interestPaid } = pastPath(ledger, day);
  const { schedule, upcoming, future } = planAhead(planFrom(ledger, day), day);

  const all = [...past, ...future];
  const payoffDay = all.find((p) => p.value <= 0)?.t ?? null;
  const principal = Math.max(0, ledger.terms.principal);
  const halfPoint = all.find((p, i) => i > 0 && p.value <= principal / 2);
  const halfDay = principal > 0 && halfPoint ? halfPoint.t : null;

  return {
    past,
    future,
    schedule,
    upcoming,
    end: schedule.end,
    balanceToday: loanBalanceAt(ledger, day).balance,
    interestPaid,
    payoffDay,
    halfDay,
  };
}

/** Value of a step series at `t`: the last reading on or before it, 0 before the first. */
export function stepValueAt(points: readonly TrajectoryPoint[], t: number): number {
  if (points.length === 0 || t < points[0].t) return 0;
  let lo = 0;
  let hi = points.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (points[mid].t <= t) lo = mid;
    else hi = mid - 1;
  }
  return points[lo].value;
}

export interface DebtSeriesInput {
  trajectory: LoanTrajectory;
  /**
   * A native balance of this loan in the series currency at the rates of day `t`; null
   * when that day cannot be converted.
   */
  convert: (amount: number, t: number) => number | null;
}

/**
 * Sum of several loan paths on a shared time grid (every reading of every loan,
 * thinned when there are many). A loan counts as 0 before its first reading, and
 * every loan that starts after the earliest one also gets a reading the day
 * before, so its drawdown is a one-day step and not a ramp from the previous
 * reading (usually the last due day, a month back). Every balance converts at its own
 * day; a reading where a non-zero balance cannot be converted is left out (the line
 * bridges it instead of dropping by that loan).
 */
export function debtSeries(items: readonly DebtSeriesInput[], today: number): TrajectoryPoint[] {
  const day = dayFloor(today);
  const series = items.map((item) => ({
    points: [...item.trajectory.past, ...item.trajectory.future],
    convert: item.convert,
  }));
  const firsts = series.flatMap((s) => (s.points.length > 0 ? [s.points[0].t] : []));
  // Every start after the earliest gets a reading the day before (the sum holds, then jumps);
  // the earliest start is where the series begins, so nothing precedes it.
  const earliest = Math.min(...firsts);
  const steps = firsts.filter((t) => t > earliest).map((t) => Math.max(earliest, t - DAY));
  const stamps = new Set<number>([day, ...steps]);
  for (const s of series) for (const p of s.points) stamps.add(p.t);
  let grid = [...stamps].sort((a, b) => a - b);
  if (grid.length > MAX_SERIES_POINTS) {
    const keep = new Set<number>([day, ...firsts, ...steps]);
    for (const item of items) {
      if (item.trajectory.payoffDay !== null) keep.add(item.trajectory.payoffDay);
    }
    const stride = Math.ceil(grid.length / MAX_SERIES_POINTS);
    grid = grid.filter((t, i) => i % stride === 0 || i === grid.length - 1 || keep.has(t));
  }
  const out: TrajectoryPoint[] = [];
  for (const t of grid) {
    let value = 0;
    let known = true;
    for (const s of series) {
      const native = stepValueAt(s.points, t);
      if (native === 0) continue;
      const converted = s.convert(native, t);
      if (converted === null) {
        known = false;
        break;
      }
      value += converted;
    }
    if (known) out.push({ t, value });
  }
  return out;
}

/**
 * The part of a series inside [start, end], with a reading on both edges so the
 * chart always spans the whole window. `start` is pulled up to the first reading.
 */
export function clipSeries(
  points: readonly TrajectoryPoint[],
  start: number,
  end: number
): TrajectoryPoint[] {
  if (points.length === 0) return [];
  const from = Math.max(start, points[0].t);
  if (end <= from) return [{ t: from, value: stepValueAt(points, from) }];
  const inside = points.filter((p) => p.t > from && p.t < end);
  return [
    { t: from, value: stepValueAt(points, from) },
    ...inside,
    { t: end, value: stepValueAt(points, end) },
  ];
}

/** Regular payment that clears `balance` in `months` payments at the annual rate (0 % = linear). */
export function annuityPayment(balance: number, annualRatePct: number, months: number): number {
  if (months <= 0 || balance <= 0) return 0;
  const r = annualRatePct / 100 / 12;
  if (r === 0) return balance / months;
  return (balance * r) / (1 - Math.pow(1 + r, -months));
}

export type ExtraPaymentMode = 'term' | 'payment';

export interface ExtraPaymentWhatIf {
  mode: ExtraPaymentMode;
  /** Balance right after the extra payment. */
  balanceAfter: number;
  /** Payments still due today, before and after. */
  monthsBefore: number;
  monthsAfter: number;
  /** Interest still due today, before and after. */
  interestBefore: number;
  interestAfter: number;
  /** The regular payment after the change (unchanged in `term` mode). */
  payment: number;
  payoffDay: number | null;
}

/**
 * What one extra payment made today would do to the plan ahead: shorten the term at the same
 * payment, or keep the payoff day and lower the payment (annuity on the reduced balance over
 * the remaining payments). `plan` is `planFrom(ledger, today)` and `balanceToday` the walk's
 * balance; the future is an estimate in either mode, only the balance of a manual loan holds.
 */
export function extraPaymentWhatIf(
  plan: LoanTerms,
  balanceToday: number,
  today: number,
  amount: number,
  mode: ExtraPaymentMode
): ExtraPaymentWhatIf | null {
  const day = dayFloor(today);
  if (plan.monthlyPayment <= 0 || balanceToday <= 0) return null;
  const base = planAhead(plan, day);
  const extra = Math.min(Math.max(0, amount), balanceToday);
  const balanceAfter = balanceToday - extra;
  const monthsBefore = base.upcoming.length;
  const interestBefore = base.upcoming.reduce((s, r) => s + r.interest, 0);
  // Rounded up to the cent so the last scheduled payment really clears the balance.
  const payment =
    mode === 'payment' && monthsBefore > 0
      ? Math.ceil(annuityPayment(balanceAfter, plan.annualRatePct, monthsBefore) * 100) / 100
      : plan.monthlyPayment;
  // Re-anchoring today moves the due days, so the contractual end would cut the last payment
  // off: run the new schedule until it is paid and report its real payoff day.
  const afterPlan: LoanTerms = {
    ...plan,
    monthlyPayment: payment,
    anchorAmount: balanceAfter,
    anchorDay: day,
    endDay: null,
  };
  const after = planAhead(afterPlan, day);
  return {
    mode,
    balanceAfter,
    monthsBefore,
    monthsAfter: after.upcoming.length,
    interestBefore,
    interestAfter: after.upcoming.reduce((s, r) => s + r.interest, 0),
    payment,
    payoffDay: planPayoffDay(afterPlan, day),
  };
}

/**
 * Input step of the extra-payment what-if: 1 % of the balance rounded down to a
 * power of ten (3 000 000 → 10 000, 120 000 → 1 000), at least 1 — the scale
 * follows the loan, whatever its currency.
 */
export function whatIfStep(balance: number): number {
  const onePercent = Math.abs(balance) * 0.01;
  if (!(onePercent >= 1)) return 1;
  return 10 ** Math.floor(Math.log10(onePercent));
}

/** A round starting extra payment: 5 % of the balance in whole steps, at least one step. */
export function suggestedExtraPayment(balance: number): number {
  const step = whatIfStep(balance);
  return Math.max(step, Math.round((balance * 0.05) / step) * step);
}
