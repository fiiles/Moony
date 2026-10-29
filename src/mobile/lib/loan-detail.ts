import type { LoanEvent, LoanSchedule } from '@shared/schema';
import {
  dayFloor,
  dueDay,
  loanAnchor,
  type AmortizationScheduleRow,
} from '@shared/calculations/loan-amortization';
import type { ChartEvent } from '@/utils/chart-scale';
import {
  clipSeries,
  stepValueAt,
  type ExtraPaymentWhatIf,
  type LoanTrajectory,
  type TrajectoryPoint,
} from '@/utils/loan-trajectory';
import type { LoanRow } from '@/utils/loans';

/**
 * The loan detail's numbers as pure functions (spec §8.8 "Úvěr"): the chart window, the marks, the
 * stats, the alerts, the schedule per year and the extra-payment sentence. They are the desktop
 * page's rules (`pages/LoanDetail.tsx`, `LoanScheduleTable.tsx`, `LoanWhatIfCard.tsx`) without its
 * strings and formatting; the phone screen words them. Every balance comes from the ledger
 * walk (`row.trajectory`), which holds a hand-entered balance exactly until it is changed.
 */

const DAY = 86_400;

/** The hero chart's segment: from the drawdown to today, 12 months back to 5 years ahead, to the end. */
export type LoanHorizon = 'past' | '5' | 'all';

export interface LoanWindow {
  start: number;
  end: number;
  /** The balance readings inside the window, one on each edge. */
  points: TrajectoryPoint[];
}

/**
 * The part of the balance path (the walk up to today, the plan after it) a horizon shows: `past`
 * ends today; `5` runs from 12 months back (not before the drawdown) to 60 months ahead; `all`
 * runs to the payoff day, or to tomorrow when the loan is never paid off.
 */
export function loanWindow(
  trajectory: LoanTrajectory,
  today: number,
  horizon: LoanHorizon
): LoanWindow {
  const series = [...trajectory.past, ...trajectory.future];
  const first = series[0]?.t ?? today;
  const last = series[series.length - 1]?.t ?? today;
  const [start, end] = (() => {
    switch (horizon) {
      case 'past':
        return [first, today];
      case '5':
        return [Math.max(first, dueDay(today, -12)), dueDay(today, 60)];
      default:
        return [first, Math.max(trajectory.payoffDay ?? last, today + DAY)];
    }
  })();
  return { start, end, points: clipSeries(series, start, end) };
}

export interface LoanChartEvent extends ChartEvent {
  kind:
    'drawdown' | 'extraPayment' | 'rateChange' | 'balanceCheck' | 'fixationEnd' | 'half' | 'payoff';
  /** The recorded event behind an extra payment, a rate change or a balance check. */
  event?: LoanEvent;
  /** The balance a milestone stands on: the fixation's day and the halfway point (half the principal). */
  balance?: number;
}

/**
 * The marks on the balance line, in day order: the drawdown, every recorded event, and the
 * milestones the loan has (the fixation end, the day half of the principal is repaid, the payoff).
 * The titles are the page's (`loans:detail.chart.events.*`). The ids are stable: `drawdown`, the
 * event's id, `fixation`, `half`, `payoff`.
 */
export function loanChartEvents(row: LoanRow, events: readonly LoanEvent[]): LoanChartEvent[] {
  const { loan, trajectory } = row;
  const series = [...trajectory.past, ...trajectory.future];
  const principal = Number(loan.principal) || 0;
  const out: LoanChartEvent[] = [
    { id: 'drawdown', t: dayFloor(loan.startDate), type: 'buy', kind: 'drawdown' },
  ];
  for (const e of events) {
    out.push({
      id: e.id,
      t: e.eventDay,
      type: e.kind === 'extra_payment' ? 'income' : 'mark',
      kind:
        e.kind === 'extra_payment'
          ? 'extraPayment'
          : e.kind === 'rate_change'
            ? 'rateChange'
            : 'balanceCheck',
      event: e,
    });
  }
  if (row.fixationEnd !== null) {
    out.push({
      id: 'fixation',
      t: row.fixationEnd,
      type: 'mark',
      kind: 'fixationEnd',
      balance: stepValueAt(series, row.fixationEnd),
    });
  }
  if (trajectory.halfDay !== null && trajectory.halfDay !== trajectory.payoffDay) {
    out.push({
      id: 'half',
      t: trajectory.halfDay,
      type: 'mark',
      kind: 'half',
      balance: principal / 2,
    });
  }
  if (trajectory.payoffDay !== null) {
    out.push({ id: 'payoff', t: trajectory.payoffDay, type: 'mark', kind: 'payoff' });
  }
  return out.sort((a, b) => a.t - b.t);
}

export interface LoanDetailStats {
  /** The drawdown day is today or past. A loan that has not started owes nothing yet and is not paid off. */
  started: boolean;
  /** The balance today from the ledger walk (0 before the drawdown). */
  balance: number;
  /** First reading of the balance path: the drawdown day. */
  startDay: number;
  /** Interest in the payments made so far (the estimated segments of the past path). */
  interestPaid: number;
  principalRepaid: number;
  /** Share of the principal repaid, 0–1. */
  repaidShare: number;
  /** Sum of the recorded extra payments. */
  extraPaid: number;
  /** Payments left; `null` when the plan never reaches 0 and does not stop at the end date either. */
  remaining: number | null;
  /** Interest still to pay in the payments ahead. */
  remainingInterest: number;
  payoffDay: number | null;
  /** The balance the plan leaves at the contractual end date (payment a bit low); else `null`. */
  endBalance: number | null;
  /** The day the rate's fixation ends, while it is still ahead of today. */
  fixationEnd: number | null;
  /** The day the rate's fixation ended, when that is today or past (the row's `fixationEnd` is then `null`). */
  fixationEnded: number | null;
  /** The next payment of the plan; `null` without one. */
  next: AmortizationScheduleRow | null;
}

/**
 * The desktop detail's hero and stat numbers (`LoanDetail.tsx`, `LoanDetailBody`). A loan that has
 * not started yet (`today` before its drawdown) has repaid nothing: 0 and 0 %, not its principal.
 */
export function loanDetailStats(
  row: LoanRow,
  events: readonly LoanEvent[],
  schedule: Pick<LoanSchedule, 'paymentsRemaining'>,
  today: number
): LoanDetailStats {
  const { loan, trajectory } = row;
  const started = dayFloor(loan.startDate) <= dayFloor(today);
  const principal = Number(loan.principal) || 0;
  const balance = trajectory.balanceToday;
  const payoffDay = trajectory.payoffDay;
  const { upcoming } = trajectory;
  // The schedule stops at the contractual end date with a balance left (the payment is a bit low)
  const endBalance =
    payoffDay === null && trajectory.end === 'endDate' && loan.endDate !== null
      ? (upcoming[upcoming.length - 1]?.balanceAfter ?? balance)
      : null;
  return {
    started,
    balance,
    startDay: trajectory.past[0]?.t ?? dayFloor(loan.startDate),
    interestPaid: trajectory.interestPaid,
    principalRepaid: started ? Math.max(0, principal - balance) : 0,
    repaidShare: started && principal > 0 ? Math.max(0, 1 - balance / principal) : 0,
    extraPaid: events
      .filter((e) => e.kind === 'extra_payment')
      .reduce((sum, e) => sum + (Number(e.amount) || 0), 0),
    remaining: schedule.paymentsRemaining ?? (endBalance !== null ? upcoming.length : null),
    remainingInterest: upcoming.reduce((sum, r) => sum + r.interest, 0),
    payoffDay,
    endBalance,
    fixationEnd: row.fixationEnd,
    // `row.fixationEnd` is null without a validity date and from the validity day on
    fixationEnded:
      loan.interestRateValidityDate !== null && row.fixationEnd === null
        ? dayFloor(loan.interestRateValidityDate)
        : null,
    next: upcoming[0] ?? null,
  };
}

/**
 * The hand-entered balance that holds today, for the note that says so: the loan's mirror of the
 * newest manual record, else the newest `balance_check` event up to today, else the plan's anchor
 * (as the desktop's valuation note falls back). `null` unless the balance is a manual one.
 */
export function manualBalance(row: LoanRow, today: number): { amount: number; day: number } | null {
  if (row.mode !== 'manual') return null;
  const { loan } = row;
  if (loan.balanceAnchorAmount !== null && loan.balanceAnchorDate !== null) {
    return {
      amount: Number(loan.balanceAnchorAmount) || 0,
      day: dayFloor(loan.balanceAnchorDate),
    };
  }
  const newest = row.ledger.events
    .filter((e) => e.kind === 'balance_check' && dayFloor(e.day) <= dayFloor(today))
    .sort((a, b) => dayFloor(b.day) - dayFloor(a.day) || b.createdAt - a.createdAt)[0];
  if (newest) return { amount: Math.max(0, newest.amount ?? 0), day: dayFloor(newest.day) };
  const anchor = loanAnchor(row.plan);
  return { amount: anchor.amount, day: anchor.day };
}

export type LoanAlert =
  | { kind: 'noPayment' }
  | { kind: 'paymentBelowInterest'; interest: number }
  | { kind: 'balanceAtEndDate'; day: number | null; amount: number }
  | { kind: 'scheduleTruncated'; count: number }
  /** The fixation ends in `days` (0 = today), on `day`. */
  | { kind: 'fixationSoon'; days: number; day: number }
  /** The fixation ended `days` ago, on `day`, and no rate change has been recorded since. */
  | { kind: 'fixationPassed'; days: number; day: number };

/** The fixation alert shows this long before the fixation ends. */
const FIXATION_SOON_DAYS = 30;
/** … and this long after it, until the new rate is written. */
const FIXATION_PASSED_DAYS = 60;

/**
 * What the screen warns about, in order: the schedule's warnings (`LoanWarnings` on the desktop —
 * a loan without a payment says so even when the schedule does not), then the rate's fixation
 * ending within 30 days, or ended up to 60 days ago with no rate change dated on or after it. A
 * loan that is paid off, past its end date or not started has no rate to write, so no fixation alert.
 */
export function loanAlerts(row: LoanRow, schedule: LoanSchedule, today: number): LoanAlert[] {
  const { loan } = row;
  const codes = [...schedule.warnings];
  if (row.mode === 'static' && !codes.includes('noPayment')) codes.unshift('noPayment');

  const alerts: LoanAlert[] = codes.flatMap((code): LoanAlert[] => {
    switch (code) {
      case 'noPayment':
        return [{ kind: 'noPayment' }];
      case 'paymentBelowInterest':
        return [
          {
            kind: 'paymentBelowInterest',
            interest: (Number(schedule.anchorAmount) * (Number(loan.interestRate) || 0)) / 100 / 12,
          },
        ];
      case 'balanceAtEndDate':
        return [
          {
            kind: 'balanceAtEndDate',
            day: loan.endDate,
            amount: Number(schedule.rows[schedule.rows.length - 1]?.balanceAfter ?? 0),
          },
        ];
      case 'scheduleTruncated':
        return [{ kind: 'scheduleTruncated', count: schedule.rows.length }];
      default:
        return [];
    }
  });

  const live = !row.matured && row.trajectory.balanceToday > 0;
  if (live && loan.interestRateValidityDate !== null) {
    const validity = dayFloor(loan.interestRateValidityDate);
    const day = dayFloor(today);
    const ahead = Math.round((validity - day) / DAY);
    if (ahead >= 0 && ahead <= FIXATION_SOON_DAYS) {
      alerts.push({ kind: 'fixationSoon', days: ahead, day: validity });
    } else if (ahead < 0 && -ahead <= FIXATION_PASSED_DAYS) {
      const answered = loan.events.some(
        (e) => e.kind === 'rate_change' && dayFloor(e.eventDay) >= validity
      );
      if (!answered) alerts.push({ kind: 'fixationPassed', days: -ahead, day: validity });
    }
  }
  return alerts;
}

/** The rows of the schedule after its valuation day, as numbers (the wire rows are text). */
export function upcomingRows(schedule: LoanSchedule): AmortizationScheduleRow[] {
  return schedule.rows
    .filter((r) => r.dueDay > schedule.asOfDay)
    .map((r) => ({
      dueDay: r.dueDay,
      payment: Number(r.payment),
      interest: Number(r.interest),
      principalPart: Number(r.principalPart),
      balanceAfter: Number(r.balanceAfter),
    }));
}

export interface ScheduleYear {
  year: number;
  count: number;
  payments: number;
  interest: number;
  principal: number;
  /** The balance after the year's last payment. */
  balanceEnd: number;
}

/** The upcoming rows per UTC calendar year (the desktop table's "By year" grouping). */
export function scheduleYears(rows: readonly AmortizationScheduleRow[]): ScheduleYear[] {
  const years = new Map<number, ScheduleYear>();
  for (const row of rows) {
    const year = new Date(row.dueDay * 1000).getUTCFullYear();
    let entry = years.get(year);
    if (!entry) {
      entry = { year, count: 0, payments: 0, interest: 0, principal: 0, balanceEnd: 0 };
      years.set(year, entry);
    }
    entry.count += 1;
    entry.payments += row.payment;
    entry.interest += row.interest;
    entry.principal += row.principalPart;
    entry.balanceEnd = row.balanceAfter;
  }
  return [...years.values()];
}

export type ExtraPaymentText =
  | { kind: 'term'; months: number; interestSaved: number }
  | { kind: 'payment'; drop: number; interestSaved: number }
  | { kind: 'noEffect' };

/**
 * What an extra payment's what-if says, as numbers for the sheet to word: the months saved in
 * `term` mode, the drop of the regular payment (`planPayment` is the plan's payment before it) in
 * `payment` mode, each with the interest saved. `null` while there is no what-if (no payment, no
 * balance), `noEffect` when neither the term, the payment nor the interest changes.
 */
export function extraPaymentText(
  whatIf: ExtraPaymentWhatIf | null,
  planPayment: number
): ExtraPaymentText | null {
  if (whatIf === null) return null;
  const interestSaved = whatIf.interestBefore - whatIf.interestAfter;
  if (whatIf.mode === 'term') {
    const months = whatIf.monthsBefore - whatIf.monthsAfter;
    return months <= 0 && interestSaved <= 0
      ? { kind: 'noEffect' }
      : { kind: 'term', months, interestSaved };
  }
  const drop = planPayment - whatIf.payment;
  return drop <= 0 && interestSaved <= 0
    ? { kind: 'noEffect' }
    : { kind: 'payment', drop, interestSaved };
}
