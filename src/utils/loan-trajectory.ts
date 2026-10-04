import {
  amortizationSchedule,
  amortizationStep,
  dayFloor,
  dueDay,
  loanAnchor,
  type AmortizationSchedule,
  type AmortizationScheduleEnd,
  type AmortizationScheduleRow,
  type LoanTerms,
} from '@shared/calculations/loan-amortization';

/** One balance reading of a loan (unix seconds, native currency). */
export interface TrajectoryPoint {
  t: number;
  value: number;
}

const DAY = 86_400;
/** The past path never walks more than a century of payments. */
const MAX_PAST_MONTHS = 1_200;
/** Combined debt series are thinned to about this many points for the chart. */
const MAX_SERIES_POINTS = 480;

export interface LoanTrajectory {
  /** Balance at the start, after every payment up to `today`, and today. */
  past: TrajectoryPoint[];
  /** Balance after every scheduled payment due after `today`. */
  future: TrajectoryPoint[];
  /** The schedule from the anchor (shared maths, same as the backend). */
  schedule: AmortizationSchedule;
  /** Rows of the schedule due after `today`. */
  upcoming: AmortizationScheduleRow[];
  end: AmortizationScheduleEnd;
  balanceToday: number;
  /** Interest in the payments made so far (estimated along the past path). */
  interestPaid: number;
  /** First day the balance reaches 0 (past or future); null when it never does. */
  payoffDay: number | null;
  /** First day the balance is at or below half the principal; null when it never is. */
  halfDay: number | null;
}

function monthlyRate(terms: LoanTerms): number {
  return terms.annualRatePct / 100 / 12;
}

/**
 * The balance path of one loan: solid past from the anchor (with the theoretical
 * path from the principal before a manual anchor, so an extra payment or a
 * statement check shows as a step on its day), dashed future from the schedule.
 */
export function loanTrajectory(terms: LoanTerms, today: number): LoanTrajectory {
  const day = dayFloor(today);
  const anchor = loanAnchor(terms);
  const rate = monthlyRate(terms);
  const past: TrajectoryPoint[] = [];
  let interestPaid = 0;

  // Before a manual anchor the stored path is unknown: amortize the principal with the
  // current terms up to the anchor, then step to the balance the user (or an event) set.
  const startDay = dayFloor(terms.startDay);
  if (anchor.isManual && anchor.day > startDay) {
    let balance = Math.max(0, terms.principal);
    past.push({ t: startDay, value: balance });
    if (terms.monthlyPayment > 0) {
      for (let k = 1; balance > 0 && k <= MAX_PAST_MONTHS; k++) {
        const due = dueDay(startDay, k);
        if (due >= anchor.day || due > day) break;
        const step = amortizationStep(balance, rate, terms.monthlyPayment);
        interestPaid += step.interest;
        balance = step.balanceAfter;
        past.push({ t: due, value: balance });
      }
    }
    if (anchor.day <= day) {
      if (anchor.day - DAY > past[past.length - 1].t) {
        past.push({ t: anchor.day - DAY, value: balance });
      }
    }
  }

  let balance = anchor.amount;
  if (anchor.day <= day) {
    past.push({ t: anchor.day, value: balance });
    if (terms.monthlyPayment > 0) {
      for (let k = 1; balance > 0 && k <= MAX_PAST_MONTHS; k++) {
        const due = dueDay(anchor.day, k);
        if (due > day) break;
        const step = amortizationStep(balance, rate, terms.monthlyPayment);
        interestPaid += step.interest;
        balance = step.balanceAfter;
        past.push({ t: due, value: balance });
      }
    }
  } else if (past.length === 0) {
    // Anchor dated in the future: nothing is known yet, hold the principal until then.
    past.push({ t: startDay, value: Math.max(0, terms.principal) });
    balance = Math.max(0, terms.principal);
  } else {
    balance = past[past.length - 1].value;
  }
  if (past[past.length - 1].t < day) past.push({ t: day, value: balance });

  const schedule = amortizationSchedule(terms);
  const upcoming = schedule.rows.filter((row) => row.dueDay > day);
  const future = upcoming.map((row) => ({ t: row.dueDay, value: row.balanceAfter }));

  const all = [...past, ...future];
  const payoffDay = all.find((p) => p.value <= 0)?.t ?? null;
  const half = Math.max(0, terms.principal) / 2;
  const halfPoint = all.find((p, i) => i > 0 && p.value <= half);
  const halfDay = terms.principal > 0 && halfPoint ? halfPoint.t : null;

  return {
    past,
    future,
    schedule,
    upcoming,
    end: schedule.end,
    balanceToday: balance,
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
  /** Multiplier from the loan's currency to the series currency. */
  rate: number;
}

/**
 * Sum of several loan paths on a shared time grid (every reading of every loan,
 * thinned when there are many). A loan counts as 0 before its first reading.
 */
export function debtSeries(items: readonly DebtSeriesInput[], today: number): TrajectoryPoint[] {
  const day = dayFloor(today);
  const series = items.map((item) => ({
    points: [...item.trajectory.past, ...item.trajectory.future],
    rate: item.rate,
  }));
  const stamps = new Set<number>([day]);
  for (const s of series) for (const p of s.points) stamps.add(p.t);
  let grid = [...stamps].sort((a, b) => a - b);
  if (grid.length > MAX_SERIES_POINTS) {
    const keep = new Set<number>([day]);
    for (const item of items) {
      const first = item.trajectory.past[0];
      if (first) keep.add(first.t);
      if (item.trajectory.payoffDay !== null) keep.add(item.trajectory.payoffDay);
    }
    const stride = Math.ceil(grid.length / MAX_SERIES_POINTS);
    grid = grid.filter((t, i) => i % stride === 0 || i === grid.length - 1 || keep.has(t));
  }
  return grid.map((t) => ({
    t,
    value: series.reduce((sum, s) => sum + stepValueAt(s.points, t) * s.rate, 0),
  }));
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
 * What one extra payment made today would do: shorten the term at the same
 * payment, or keep the payoff day and lower the payment (annuity on the reduced
 * balance over the remaining payments).
 */
export function extraPaymentWhatIf(
  terms: LoanTerms,
  today: number,
  amount: number,
  mode: ExtraPaymentMode
): ExtraPaymentWhatIf | null {
  const day = dayFloor(today);
  const base = loanTrajectory(terms, day);
  if (terms.monthlyPayment <= 0 || base.balanceToday <= 0) return null;
  const extra = Math.min(Math.max(0, amount), base.balanceToday);
  const balanceAfter = base.balanceToday - extra;
  const monthsBefore = base.upcoming.length;
  const interestBefore = base.upcoming.reduce((s, r) => s + r.interest, 0);
  // Rounded up to the cent so the last scheduled payment really clears the balance.
  const payment =
    mode === 'payment' && monthsBefore > 0
      ? Math.ceil(annuityPayment(balanceAfter, terms.annualRatePct, monthsBefore) * 100) / 100
      : terms.monthlyPayment;
  // Re-anchoring today moves the due days, so the contractual end would cut the last payment
  // off: run the new schedule until it is paid and report its real payoff day.
  const after = loanTrajectory(
    {
      ...terms,
      monthlyPayment: payment,
      anchorAmount: balanceAfter,
      anchorDay: day,
      endDay: null,
    },
    day
  );
  return {
    mode,
    balanceAfter,
    monthsBefore,
    monthsAfter: after.upcoming.length,
    interestBefore,
    interestAfter: after.upcoming.reduce((s, r) => s + r.interest, 0),
    payment,
    payoffDay: after.payoffDay,
  };
}
