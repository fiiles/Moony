import { describe, expect, it } from 'vitest';
import type { Loan, LoanEvent, LoanSchedule } from '@shared/schema';
import { dueDay, loanBalanceAt } from '@shared/calculations/loan-amortization';
import {
  annuityPayment,
  extraPaymentWhatIf,
  type ExtraPaymentWhatIf,
} from '@/utils/loan-trajectory';
import { loanRow, type LoanRow } from '@/utils/loans';
import {
  extraPaymentText,
  loanAlerts,
  loanChartEvents,
  loanDetailStats,
  loanWindow,
  manualBalance,
  scheduleYears,
  upcomingRows,
} from './loan-detail';

const DAY = 86_400;
const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;
const TODAY = day(2026, 10, 3);
const START = day(2019, 5, 15);
const RECORD = day(2026, 3, 3);
const convert = (amount: number) => amount;

/** The payment that clears 3 100 000 at 1,9 % in 360 payments, rounded up to the cent. */
const PAYMENT = Math.ceil(annuityPayment(3_100_000, 1.9, 360) * 100) / 100;

function loan(partial: Partial<Loan> = {}): Loan {
  return {
    id: 'l1',
    name: 'Hypotéka Lipová',
    principal: '3100000',
    currency: 'CZK',
    interestRate: '1.9',
    interestRateValidityDate: null,
    monthlyPayment: String(PAYMENT),
    startDate: START,
    endDate: day(2049, 5, 15),
    balanceAnchorAmount: null,
    balanceAnchorDate: null,
    outstandingBalance: '1000000.00',
    balanceMode: 'estimated',
    events: [],
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}

function event(partial: Partial<LoanEvent> & Pick<LoanEvent, 'kind' | 'eventDay'>): LoanEvent {
  return {
    id: `e-${partial.kind}-${partial.eventDay}`,
    loanId: 'l1',
    amount: null,
    rate: null,
    monthlyPayment: null,
    previousRate: null,
    previousMonthlyPayment: null,
    note: null,
    createdAt: 1,
    ...partial,
  };
}

const balanceCheck = event({ kind: 'balance_check', eventDay: RECORD, amount: '2400000' });

const rowOf = (partial: Partial<Loan> = {}) => loanRow(loan(partial), TODAY, convert);

/** The wire schedule the backend would send for a row, with the warnings given. */
function scheduleOf(
  row: LoanRow,
  partial: Partial<LoanSchedule> = {},
  rows: LoanRow['trajectory']['schedule']['rows'] = row.trajectory.schedule.rows
): LoanSchedule {
  return {
    loanId: row.loan.id,
    asOfDay: TODAY,
    anchorAmount: String(row.plan.anchorAmount ?? row.plan.principal),
    anchorDay: row.plan.anchorDay ?? row.plan.startDay,
    anchorIsManual: row.mode === 'manual',
    outstandingBalance: String(row.trajectory.balanceToday),
    principalRepaid: '0',
    interestPaid: '0',
    paymentsMade: 0,
    paymentsRemaining: row.trajectory.upcoming.length,
    payoffDay: row.trajectory.payoffDay,
    rows: rows.map((r) => ({
      dueDay: r.dueDay,
      payment: String(r.payment),
      interest: String(r.interest),
      principalPart: String(r.principalPart),
      balanceAfter: String(r.balanceAfter),
    })),
    warnings: [],
    ...partial,
  };
}

describe('loanWindow', () => {
  const row = rowOf();

  it("'past' runs from the drawdown to today", () => {
    const w = loanWindow(row.trajectory, TODAY, 'past');
    expect(w.start).toBe(START);
    expect(w.end).toBe(TODAY);
    expect(w.points[0].t).toBe(START);
    expect(w.points[w.points.length - 1].t).toBe(TODAY);
    expect(w.points.every((p) => p.t <= TODAY)).toBe(true);
  });

  it("'5' runs from 12 months back to 60 months ahead, both edges carry a reading", () => {
    const w = loanWindow(row.trajectory, TODAY, '5');
    expect(w.start).toBe(dueDay(TODAY, -12));
    expect(w.end).toBe(dueDay(TODAY, 60));
    expect(w.points[0].t).toBe(w.start);
    expect(w.points[w.points.length - 1].t).toBe(w.end);
    expect(w.points.some((p) => p.t > TODAY)).toBe(true);
  });

  it("'5' starts at the drawdown for a loan younger than 12 months", () => {
    const young = rowOf({ startDate: day(2026, 6, 15), endDate: null, principal: '1000000' });
    const w = loanWindow(young.trajectory, TODAY, '5');
    expect(w.start).toBe(day(2026, 6, 15));
  });

  it("'all' ends at the payoff day", () => {
    const w = loanWindow(row.trajectory, TODAY, 'all');
    expect(row.trajectory.payoffDay).toBe(day(2049, 5, 15));
    expect(w.start).toBe(START);
    expect(w.end).toBe(day(2049, 5, 15));
  });

  it("'all' ends a day after today when the balance never reaches 0", () => {
    const stuck = rowOf({ monthlyPayment: '0' });
    expect(stuck.trajectory.payoffDay).toBeNull();
    const w = loanWindow(stuck.trajectory, TODAY, 'all');
    expect(w.end).toBe(TODAY + DAY);
  });

  it('holds a manual balance flat from the record to today', () => {
    const manual = rowOf({
      balanceAnchorAmount: '2400000',
      balanceAnchorDate: RECORD,
      outstandingBalance: '2400000.00',
      balanceMode: 'manual',
      events: [balanceCheck],
    });
    expect(manual.mode).toBe('manual');
    const w = loanWindow(manual.trajectory, TODAY, 'past');
    const after = w.points.filter((p) => p.t >= RECORD);
    // The record's own day and today at least, every reading exactly the entered balance
    expect(after.length).toBeGreaterThanOrEqual(2);
    expect(after.every((p) => p.value === 2_400_000)).toBe(true);
    // Three payment days passed since the record (15. 4., 15. 5., 15. 6., …): nothing amortized
    expect(loanBalanceAt(manual.ledger, day(2026, 6, 20)).balance).toBe(2_400_000);
    expect(loanBalanceAt(manual.ledger, TODAY).balance).toBe(2_400_000);
  });

  it('has only the drawdown reading in the past of a loan that has not started', () => {
    const later = rowOf({ startDate: day(2026, 12, 1), endDate: null, principal: '1000000' });
    const past = loanWindow(later.trajectory, TODAY, 'past');
    expect(past.points).toHaveLength(1);
    expect(past.points[0].t).toBe(day(2026, 12, 1));
    // The 5-year window starts at the drawdown, which is ahead of today, and runs on as the plan
    const five = loanWindow(later.trajectory, TODAY, '5');
    expect(five.start).toBe(day(2026, 12, 1));
    expect(five.end).toBe(dueDay(TODAY, 60));
    expect(five.points[0]).toEqual({ t: day(2026, 12, 1), value: 1_000_000 });
    expect(five.points.every((p) => p.t >= day(2026, 12, 1))).toBe(true);
  });
});

describe('loanChartEvents', () => {
  const extra = event({ kind: 'extra_payment', eventDay: day(2024, 3, 3), amount: '50000' });
  const rate = event({ kind: 'rate_change', eventDay: day(2025, 5, 15), rate: '4.1' });

  it('has the drawdown first as a buy at the start', () => {
    const marks = loanChartEvents(rowOf(), []);
    expect(marks[0]).toMatchObject({ id: 'drawdown', kind: 'drawdown', type: 'buy', t: START });
  });

  it('types the extra payments as income and the rate changes and balance checks as marks', () => {
    const row = rowOf({ events: [extra, rate, balanceCheck] });
    const marks = loanChartEvents(row, [extra, rate, balanceCheck]);
    const by = (id: string) => marks.find((m) => m.id === id);
    expect(by(extra.id)).toMatchObject({ kind: 'extraPayment', type: 'income', t: extra.eventDay });
    expect(by(extra.id)?.event).toBe(extra);
    expect(by(rate.id)).toMatchObject({ kind: 'rateChange', type: 'mark' });
    expect(by(balanceCheck.id)).toMatchObject({ kind: 'balanceCheck', type: 'mark' });
    expect(by('drawdown')?.event).toBeUndefined();
  });

  it('adds the fixation end, the halfway point and the payoff when the row has them', () => {
    const row = rowOf({ interestRateValidityDate: day(2026, 10, 20) });
    const marks = loanChartEvents(row, []);
    expect(marks.map((m) => m.id)).toEqual(['drawdown', 'fixation', 'half', 'payoff']);
    const fixation = marks.find((m) => m.id === 'fixation');
    expect(fixation).toMatchObject({ kind: 'fixationEnd', type: 'mark', t: day(2026, 10, 20) });
    expect(fixation?.balance).toBeGreaterThan(0);
    const half = marks.find((m) => m.id === 'half');
    expect(half).toMatchObject({ kind: 'half', type: 'mark', balance: 1_550_000 });
    expect(half?.t).toBe(row.trajectory.halfDay);
    expect(marks.find((m) => m.id === 'payoff')).toMatchObject({
      kind: 'payoff',
      type: 'mark',
      t: day(2049, 5, 15),
    });
  });

  it('leaves out the milestones a loan has not got', () => {
    const marks = loanChartEvents(rowOf({ monthlyPayment: '0' }), []);
    expect(marks.map((m) => m.id)).toEqual(['drawdown']);
  });

  it('is in day order', () => {
    const row = rowOf({ events: [extra, rate], interestRateValidityDate: day(2026, 10, 20) });
    const marks = loanChartEvents(row, [rate, extra]);
    const ts = marks.map((m) => m.t);
    expect(ts).toEqual([...ts].sort((a, b) => a - b));
  });
});

describe('loanDetailStats', () => {
  it("takes the desktop's numbers for an automatic loan", () => {
    const row = rowOf();
    const schedule = scheduleOf(row);
    const stats = loanDetailStats(row, [], schedule, TODAY);
    const { trajectory } = row;
    expect(stats.balance).toBe(trajectory.balanceToday);
    expect(stats.interestPaid).toBe(trajectory.interestPaid);
    expect(stats.principalRepaid).toBeCloseTo(3_100_000 - trajectory.balanceToday, 6);
    expect(stats.repaidShare).toBeCloseTo(1 - trajectory.balanceToday / 3_100_000, 9);
    expect(stats.extraPaid).toBe(0);
    expect(stats.remaining).toBe(trajectory.upcoming.length);
    expect(stats.payoffDay).toBe(day(2049, 5, 15));
    expect(stats.endBalance).toBeNull();
    expect(stats.fixationEnd).toBeNull();
    expect(stats.next).toBe(trajectory.upcoming[0]);
    expect(stats.next?.dueDay).toBe(day(2026, 10, 15));
    expect(stats.startDay).toBe(START);
    expect(stats.remainingInterest).toBeCloseTo(
      trajectory.upcoming.reduce((s, r) => s + r.interest, 0),
      6
    );
  });

  it('sums the extra payments', () => {
    const extra = [
      event({ kind: 'extra_payment', eventDay: day(2024, 3, 3), amount: '50000' }),
      event({ kind: 'extra_payment', eventDay: day(2025, 3, 3), amount: '25000.50' }),
      event({ kind: 'rate_change', eventDay: day(2025, 5, 15), rate: '4.1' }),
    ];
    const row = rowOf({ events: extra });
    const stats = loanDetailStats(row, extra, scheduleOf(row), TODAY);
    expect(stats.extraPaid).toBeCloseTo(75_000.5, 6);
  });

  it('keeps a hand-entered balance exactly: the balance, the repaid share and the plan start from it', () => {
    const row = rowOf({
      balanceAnchorAmount: '2400000',
      balanceAnchorDate: RECORD,
      outstandingBalance: '2400000.00',
      balanceMode: 'manual',
      events: [balanceCheck],
    });
    const stats = loanDetailStats(row, [balanceCheck], scheduleOf(row), TODAY);
    expect(stats.balance).toBe(loanBalanceAt(row.ledger, TODAY).balance);
    expect(stats.balance).toBe(2_400_000);
    expect(stats.principalRepaid).toBe(700_000);
    expect(stats.repaidShare).toBeCloseTo(700_000 / 3_100_000, 9);
    // The plan ahead continues from the entered balance, not from the old estimate
    expect(row.plan.anchorAmount).toBe(2_400_000);
    expect(stats.next?.balanceAfter).toBeLessThan(2_400_000);
  });

  it('counts the schedule rows as the payments left when it stops at the end date with a balance', () => {
    const short = rowOf({
      principal: '1000000',
      interestRate: '5',
      monthlyPayment: '5000',
      startDate: day(2026, 1, 15),
      endDate: day(2046, 1, 15),
    });
    expect(short.trajectory.end).toBe('endDate');
    expect(short.trajectory.payoffDay).toBeNull();
    const stats = loanDetailStats(short, [], scheduleOf(short, { paymentsRemaining: null }), TODAY);
    const last = short.trajectory.upcoming[short.trajectory.upcoming.length - 1];
    expect(stats.endBalance).toBeCloseTo(last.balanceAfter, 6);
    expect(stats.endBalance).toBeGreaterThan(0);
    expect(stats.remaining).toBe(short.trajectory.upcoming.length);
    expect(stats.payoffDay).toBeNull();
  });

  it('has no payments left to count for a loan without a payment', () => {
    const stuck = rowOf({ monthlyPayment: '0' });
    const stats = loanDetailStats(stuck, [], scheduleOf(stuck, { paymentsRemaining: null }), TODAY);
    expect(stats.remaining).toBeNull();
    expect(stats.next).toBeNull();
    expect(stats.endBalance).toBeNull();
    expect(stats.balance).toBe(3_100_000);
  });

  it('gives the fixation end of the row', () => {
    const row = rowOf({ interestRateValidityDate: day(2026, 10, 13) });
    const stats = loanDetailStats(row, [], scheduleOf(row), TODAY);
    expect(stats.fixationEnd).toBe(day(2026, 10, 13));
    expect(stats.fixationEnded).toBeNull();
  });

  it('tells a fixation that has ended (today or past) from a loan without one', () => {
    const none = rowOf();
    const noneStats = loanDetailStats(none, [], scheduleOf(none), TODAY);
    expect(noneStats.fixationEnd).toBeNull();
    expect(noneStats.fixationEnded).toBeNull();
    for (const validity of [TODAY, TODAY - 40 * DAY]) {
      const row = rowOf({ interestRateValidityDate: validity });
      const stats = loanDetailStats(row, [], scheduleOf(row), TODAY);
      expect(stats.fixationEnd).toBeNull();
      expect(stats.fixationEnded).toBe(validity);
    }
  });

  it('has repaid nothing before the drawdown: 0 and 0 %, not the whole principal', () => {
    const later = rowOf({ startDate: day(2026, 12, 1), endDate: null, principal: '1000000' });
    const stats = loanDetailStats(later, [], scheduleOf(later), TODAY);
    expect(stats.started).toBe(false);
    expect(stats.balance).toBe(0);
    expect(stats.principalRepaid).toBe(0);
    expect(stats.repaidShare).toBe(0);
    expect(stats.interestPaid).toBe(0);
    expect(stats.startDay).toBe(day(2026, 12, 1));
    // A loan that has started is `started`, a paid-off one too
    expect(loanDetailStats(rowOf(), [], scheduleOf(rowOf()), TODAY).started).toBe(true);
    const paid = rowOf({ outstandingBalance: '0' });
    expect(loanDetailStats(paid, [], scheduleOf(paid), TODAY).started).toBe(true);
    expect(loanDetailStats(later, [], scheduleOf(later), day(2026, 12, 1)).started).toBe(true);
  });
});

describe('manualBalance', () => {
  const manualLoan = {
    balanceAnchorAmount: '2400000',
    balanceAnchorDate: RECORD,
    outstandingBalance: '2400000.00',
    balanceMode: 'manual' as const,
    events: [balanceCheck],
  };

  it('is the mirror of the newest record while the balance is a manual one', () => {
    expect(manualBalance(rowOf(manualLoan), TODAY)).toEqual({ amount: 2_400_000, day: RECORD });
  });

  it('is nothing for an estimated balance', () => {
    expect(manualBalance(rowOf(), TODAY)).toBeNull();
    expect(manualBalance(rowOf({ monthlyPayment: '0' }), TODAY)).toBeNull();
  });

  it('falls back to the newest record of the ledger when the mirror is missing', () => {
    const older = event({ kind: 'balance_check', eventDay: day(2025, 11, 20), amount: '2900000' });
    const row = rowOf({
      ...manualLoan,
      balanceAnchorAmount: null,
      balanceAnchorDate: null,
      events: [older, balanceCheck],
    });
    expect(row.mode).toBe('manual');
    expect(manualBalance(row, TODAY)).toEqual({ amount: 2_400_000, day: RECORD });
    // A record dated after today does not hold today
    const future = event({ kind: 'balance_check', eventDay: TODAY + 5 * DAY, amount: '1000000' });
    const withFuture = rowOf({
      ...manualLoan,
      balanceAnchorAmount: null,
      balanceAnchorDate: null,
      events: [balanceCheck, future],
    });
    expect(manualBalance(withFuture, TODAY)).toEqual({ amount: 2_400_000, day: RECORD });
  });
});

describe('loanAlerts', () => {
  it('has none for a healthy loan', () => {
    const row = rowOf();
    expect(loanAlerts(row, scheduleOf(row), TODAY)).toEqual([]);
  });

  it('says there is no payment for a loan without one, once', () => {
    const stuck = rowOf({ monthlyPayment: '0' });
    expect(stuck.mode).toBe('static');
    expect(loanAlerts(stuck, scheduleOf(stuck), TODAY)).toEqual([{ kind: 'noPayment' }]);
    expect(loanAlerts(stuck, scheduleOf(stuck, { warnings: ['noPayment'] }), TODAY)).toEqual([
      { kind: 'noPayment' },
    ]);
  });

  it("turns the schedule's warnings into alerts, in the schedule's order", () => {
    const low = rowOf({ monthlyPayment: '4000', endDate: day(2046, 5, 15) });
    expect(low.belowInterest).toBe(true);
    const schedule = scheduleOf(low, {
      anchorAmount: '3100000',
      warnings: ['paymentBelowInterest', 'balanceAtEndDate', 'scheduleTruncated'],
    });
    const alerts = loanAlerts(low, schedule, TODAY);
    expect(alerts.map((a) => a.kind)).toEqual([
      'paymentBelowInterest',
      'balanceAtEndDate',
      'scheduleTruncated',
    ]);
    const below = alerts[0];
    // 3 100 000 × 1,9 % ÷ 12
    expect(below.kind === 'paymentBelowInterest' && below.interest).toBeCloseTo(4_908.33, 2);
    const end = alerts[1];
    expect(end.kind === 'balanceAtEndDate' && end.day).toBe(day(2046, 5, 15));
    expect(end.kind === 'balanceAtEndDate' && end.amount).toBeCloseTo(
      low.trajectory.schedule.rows[low.trajectory.schedule.rows.length - 1]?.balanceAfter ?? 0,
      6
    );
    const truncated = alerts[2];
    expect(truncated.kind === 'scheduleTruncated' && truncated.count).toBe(schedule.rows.length);
  });

  it('ignores a warning code it does not know', () => {
    const row = rowOf();
    expect(loanAlerts(row, scheduleOf(row, { warnings: ['somethingNew'] }), TODAY)).toEqual([]);
  });

  it('warns that the fixation ends 0 to 30 days ahead', () => {
    const soon = (ahead: number) => rowOf({ interestRateValidityDate: TODAY + ahead * DAY });
    const alertsFor = (ahead: number) => {
      const row = soon(ahead);
      return loanAlerts(row, scheduleOf(row), TODAY);
    };
    expect(alertsFor(10)).toEqual([{ kind: 'fixationSoon', days: 10, day: TODAY + 10 * DAY }]);
    expect(alertsFor(30)).toEqual([{ kind: 'fixationSoon', days: 30, day: TODAY + 30 * DAY }]);
    expect(alertsFor(0)).toEqual([{ kind: 'fixationSoon', days: 0, day: TODAY }]);
    expect(alertsFor(31)).toEqual([]);
  });

  it('reminds to enter the new rate up to 60 days after the fixation, unless one was recorded', () => {
    const validity = TODAY - 20 * DAY;
    const passed = rowOf({ interestRateValidityDate: validity });
    expect(loanAlerts(passed, scheduleOf(passed), TODAY)).toEqual([
      { kind: 'fixationPassed', days: 20, day: validity },
    ]);

    const handled = event({ kind: 'rate_change', eventDay: validity + DAY, rate: '4.1' });
    const answered = rowOf({ interestRateValidityDate: validity, events: [handled] });
    expect(loanAlerts(answered, scheduleOf(answered), TODAY)).toEqual([]);

    // A rate change before the fixation ended does not answer it
    const earlier = event({ kind: 'rate_change', eventDay: validity - DAY, rate: '3.1' });
    const unanswered = rowOf({ interestRateValidityDate: validity, events: [earlier] });
    expect(loanAlerts(unanswered, scheduleOf(unanswered), TODAY).map((a) => a.kind)).toEqual([
      'fixationPassed',
    ]);

    const old = rowOf({ interestRateValidityDate: TODAY - 61 * DAY });
    expect(loanAlerts(old, scheduleOf(old), TODAY)).toEqual([]);
    const edge = rowOf({ interestRateValidityDate: TODAY - 60 * DAY });
    expect(loanAlerts(edge, scheduleOf(edge), TODAY).map((a) => a.kind)).toEqual([
      'fixationPassed',
    ]);
  });

  it('has no fixation alert for a loan that is paid off, past its end date or not started', () => {
    const soon = TODAY + 5 * DAY;
    const paid = rowOf({ outstandingBalance: '0', interestRateValidityDate: soon });
    expect(paid.matured).toBe(true);
    expect(loanAlerts(paid, scheduleOf(paid), TODAY)).toEqual([]);

    const ended = rowOf({ endDate: day(2026, 1, 15), interestRateValidityDate: soon });
    expect(ended.matured).toBe(true);
    expect(loanAlerts(ended, scheduleOf(ended), TODAY)).toEqual([]);

    const later = rowOf({
      startDate: day(2026, 12, 1),
      endDate: null,
      interestRateValidityDate: soon,
    });
    expect(loanAlerts(later, scheduleOf(later), TODAY)).toEqual([]);

    // The same fixation on a loan that is running still alerts
    const live = rowOf({ interestRateValidityDate: soon });
    expect(loanAlerts(live, scheduleOf(live), TODAY).map((a) => a.kind)).toEqual(['fixationSoon']);
  });

  it('lists the schedule warnings before the fixation alert', () => {
    const stuck = rowOf({ monthlyPayment: '0', interestRateValidityDate: TODAY + 5 * DAY });
    expect(loanAlerts(stuck, scheduleOf(stuck), TODAY).map((a) => a.kind)).toEqual([
      'noPayment',
      'fixationSoon',
    ]);
  });
});

describe('upcomingRows and scheduleYears', () => {
  const row = rowOf();
  const schedule = scheduleOf(row);

  it('takes the rows after the valuation day as numbers', () => {
    const rows = upcomingRows({
      ...schedule,
      rows: [
        {
          dueDay: TODAY - DAY,
          payment: '10',
          interest: '4',
          principalPart: '6',
          balanceAfter: '90',
        },
        { dueDay: TODAY, payment: '10', interest: '4', principalPart: '6', balanceAfter: '84' },
        {
          dueDay: TODAY + DAY,
          payment: '10',
          interest: '3',
          principalPart: '7',
          balanceAfter: '77',
        },
      ],
    });
    expect(rows).toEqual([
      { dueDay: TODAY + DAY, payment: 10, interest: 3, principalPart: 7, balanceAfter: 77 },
    ]);
  });

  it('groups the upcoming rows per UTC calendar year', () => {
    const years = scheduleYears(row.trajectory.upcoming);
    expect(years[0].year).toBe(2026);
    // 15. 10., 15. 11. and 15. 12.
    expect(years[0].count).toBe(3);
    expect(years[1]).toMatchObject({ year: 2027, count: 12 });
    expect(years[years.length - 1]).toMatchObject({ year: 2049, count: 5 });
    expect(years.map((y) => y.year)).toEqual(
      Array.from({ length: 2049 - 2026 + 1 }, (_, i) => 2026 + i)
    );
    expect(years.reduce((s, y) => s + y.count, 0)).toBe(row.trajectory.upcoming.length);
  });

  it("sums a year's payments, interest and principal and ends on its last balance", () => {
    const years = scheduleYears(row.trajectory.upcoming);
    const rows2027 = row.trajectory.upcoming.filter(
      (r) => new Date(r.dueDay * 1000).getUTCFullYear() === 2027
    );
    const y2027 = years.find((y) => y.year === 2027)!;
    expect(y2027.payments).toBeCloseTo(
      rows2027.reduce((s, r) => s + r.payment, 0),
      6
    );
    expect(y2027.interest).toBeCloseTo(
      rows2027.reduce((s, r) => s + r.interest, 0),
      6
    );
    expect(y2027.principal).toBeCloseTo(
      rows2027.reduce((s, r) => s + r.principalPart, 0),
      6
    );
    expect(y2027.balanceEnd).toBe(rows2027[rows2027.length - 1].balanceAfter);
    expect(years[years.length - 1].balanceEnd).toBe(0);
  });

  it('gives no years for no rows', () => {
    expect(scheduleYears([])).toEqual([]);
  });
});

describe('extraPaymentText', () => {
  const row = rowOf();
  const balance = row.trajectory.balanceToday;

  it('is null while there is no what-if', () => {
    expect(extraPaymentText(null, row.plan.monthlyPayment)).toBeNull();
  });

  it('words the months saved in term mode', () => {
    const whatIf = extraPaymentWhatIf(row.plan, balance, TODAY, 200_000, 'term');
    const text = extraPaymentText(whatIf, row.plan.monthlyPayment);
    expect(text?.kind).toBe('term');
    if (text?.kind !== 'term' || !whatIf) throw new Error('expected a term result');
    expect(text.months).toBe(whatIf.monthsBefore - whatIf.monthsAfter);
    expect(text.months).toBeGreaterThan(0);
    expect(text.interestSaved).toBeCloseTo(whatIf.interestBefore - whatIf.interestAfter, 6);
    expect(text.interestSaved).toBeGreaterThan(0);
  });

  it('words the drop of the payment in payment mode', () => {
    const whatIf = extraPaymentWhatIf(row.plan, balance, TODAY, 200_000, 'payment');
    const text = extraPaymentText(whatIf, row.plan.monthlyPayment);
    expect(text?.kind).toBe('payment');
    if (text?.kind !== 'payment' || !whatIf) throw new Error('expected a payment result');
    expect(text.drop).toBeCloseTo(row.plan.monthlyPayment - whatIf.payment, 6);
    expect(text.drop).toBeGreaterThan(0);
    expect(text.interestSaved).toBeCloseTo(whatIf.interestBefore - whatIf.interestAfter, 6);
  });

  it('says nothing changes when it does not', () => {
    const same: ExtraPaymentWhatIf = {
      mode: 'term',
      balanceAfter: 100,
      monthsBefore: 12,
      monthsAfter: 12,
      interestBefore: 50,
      interestAfter: 50,
      payment: 10,
      payoffDay: TODAY,
    };
    expect(extraPaymentText(same, 10)).toEqual({ kind: 'noEffect' });
    expect(extraPaymentText({ ...same, mode: 'payment' }, 10)).toEqual({ kind: 'noEffect' });
  });

  it('is null for a loan without a payment, whose what-if is null', () => {
    const stuck = rowOf({ monthlyPayment: '0' });
    const whatIf = extraPaymentWhatIf(
      stuck.plan,
      stuck.trajectory.balanceToday,
      TODAY,
      50_000,
      'term'
    );
    expect(whatIf).toBeNull();
    expect(extraPaymentText(whatIf, stuck.plan.monthlyPayment)).toBeNull();
  });
});
