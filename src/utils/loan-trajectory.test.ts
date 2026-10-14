import { describe, expect, it } from 'vitest';
import {
  loanBalanceAt,
  planFrom,
  type LedgerEvent,
  type LoanLedger,
  type LoanTerms,
} from '@shared/calculations/loan-amortization';
import {
  annuityPayment,
  clipSeries,
  debtSeries,
  extraPaymentWhatIf,
  loanTrajectory as trajectoryOf,
  planPayoffDay,
  stepValueAt,
  suggestedExtraPayment,
  whatIfStep,
} from './loan-trajectory';

const DAY = 86_400;
/** 2024-01-01 UTC. */
const START = Date.UTC(2024, 0, 1) / 1000;
const day = (y: number, m: number, d: number) => Date.UTC(y, m - 1, d) / 1000;

const zeroRate: LoanTerms = {
  principal: 120_000,
  annualRatePct: 0,
  monthlyPayment: 10_000,
  startDay: START,
};

const ledgerOf = (terms: LoanTerms, events: LedgerEvent[] = []): LoanLedger => ({ terms, events });
/** A manual balance record (the stored `balance_check` event). */
const manual = (on: number, amount: number, createdAt = 1): LedgerEvent => ({
  day: on,
  createdAt,
  kind: 'balance_check',
  amount,
});
const extra = (on: number, amount: number, createdAt = 1): LedgerEvent => ({
  day: on,
  createdAt,
  kind: 'extra_payment',
  amount,
});
/** The trajectory of loan terms without events, the common case of these tests. */
const loanTrajectory = (terms: LoanTerms, today: number) => trajectoryOf(ledgerOf(terms), today);

describe('loanTrajectory', () => {
  it('walks the balance down one payment a month and continues with the schedule', () => {
    const today = day(2024, 6, 15);
    const traj = loanTrajectory(zeroRate, today);
    // start + 5 payments (Feb..Jun) + today
    expect(traj.past[0]).toEqual({ t: START, value: 120_000 });
    expect(traj.past[traj.past.length - 1]).toEqual({ t: today, value: 70_000 });
    expect(traj.balanceToday).toBe(70_000);
    expect(traj.upcoming).toHaveLength(7);
    expect(traj.future[0]).toEqual({ t: day(2024, 7, 1), value: 60_000 });
    expect(traj.payoffDay).toBe(day(2025, 1, 1));
    expect(traj.halfDay).toBe(day(2024, 7, 1));
    expect(traj.end).toBe('paid');
  });

  it('steps to a manual balance on its day and keeps the estimated path before it', () => {
    const recordDay = day(2024, 4, 1);
    const traj = trajectoryOf(ledgerOf(zeroRate, [manual(recordDay, 50_000)]), day(2024, 4, 20));
    const before = traj.past.find((p) => p.t === recordDay - DAY);
    const at = traj.past.find((p) => p.t === recordDay);
    // Feb and Mar payments were made on the estimated path: 100 000 left
    expect(before?.value).toBe(100_000);
    expect(at?.value).toBe(50_000);
    expect(traj.balanceToday).toBe(50_000);
    // The plan ahead continues from the manual balance: May to Sep
    expect(traj.payoffDay).toBe(day(2024, 9, 1));
    expect(traj.future[0]).toEqual({ t: day(2024, 5, 1), value: 40_000 });
  });

  it('holds a manual balance flat: nothing is amortized after it, and every record is a step', () => {
    const first = day(2024, 3, 10);
    const second = day(2024, 7, 20);
    const today = day(2024, 9, 5);
    const traj = trajectoryOf(
      ledgerOf(zeroRate, [manual(first, 90_000), manual(second, 60_000, 2)]),
      today
    );
    const at = (t: number) => stepValueAt(traj.past, t);
    // Feb and Mar are paid on the estimated path before the first record
    expect(at(first - DAY)).toBe(100_000);
    expect(at(first)).toBe(90_000);
    // Payments due between the records change nothing
    expect(at(day(2024, 5, 1))).toBe(90_000);
    expect(at(second - DAY)).toBe(90_000);
    expect(at(second)).toBe(60_000);
    expect(at(today)).toBe(60_000);
    expect(traj.balanceToday).toBe(60_000);
    // No due-day reading inside a manual stretch: one flat line from the step to the next
    expect(traj.past.filter((p) => p.t > first && p.t < second - DAY)).toHaveLength(0);
  });

  it('counts only the interest of the estimated payments', () => {
    const terms = { principal: 100_000, annualRatePct: 12, monthlyPayment: 2_000, startDay: START };
    const estimated = trajectoryOf(ledgerOf(terms), day(2024, 3, 15));
    expect(estimated.interestPaid).toBeCloseTo(1_990, 2);
    // A manual balance on 1 Feb stops the amortization: the one payment before it was estimated
    const held = trajectoryOf(ledgerOf(terms, [manual(day(2024, 2, 20), 90_000)]), day(2024, 6, 1));
    expect(held.interestPaid).toBeCloseTo(1_000, 2);
  });

  it('shows an extra payment as a step and carries on amortizing from it', () => {
    const on = day(2024, 3, 20);
    const traj = trajectoryOf(ledgerOf(zeroRate, [extra(on, 5_000)]), day(2024, 5, 15));
    expect(stepValueAt(traj.past, on - DAY)).toBe(100_000);
    expect(stepValueAt(traj.past, on)).toBe(95_000);
    // The new cycle is the 20th: 20 Apr is the next payment
    expect(stepValueAt(traj.past, day(2024, 4, 20))).toBe(85_000);
    expect(traj.balanceToday).toBe(85_000);
  });

  it('keeps the payment due on an event day inside the old segment', () => {
    // A payment is due on 1 Mar, the day of the record: the step is drawn from the day before
    const recordDay = day(2024, 3, 1);
    const traj = trajectoryOf(ledgerOf(zeroRate, [manual(recordDay, 70_000)]), day(2024, 3, 15));
    expect(stepValueAt(traj.past, recordDay - DAY)).toBe(110_000);
    expect(stepValueAt(traj.past, recordDay)).toBe(70_000);
    expect(traj.past.filter((p) => p.t === recordDay)).toHaveLength(1);
  });

  it('has only the start reading for a loan that starts after today', () => {
    const traj = loanTrajectory({ ...zeroRate, startDay: day(2024, 9, 1) }, day(2024, 6, 15));
    expect(traj.past).toEqual([{ t: day(2024, 9, 1), value: 120_000 }]);
    expect(traj.balanceToday).toBe(0);
    expect(traj.payoffDay).toBe(day(2025, 9, 1));
  });

  it('accumulates the interest of the payments made so far', () => {
    // 1 % a month on 100 000, 2 000 a month: 1 000 + 990 of interest after two payments
    const traj = loanTrajectory(
      { principal: 100_000, annualRatePct: 12, monthlyPayment: 2_000, startDay: START },
      day(2024, 3, 15)
    );
    expect(traj.interestPaid).toBeCloseTo(1_990, 2);
    expect(loanTrajectory(zeroRate, day(2024, 6, 15)).interestPaid).toBe(0);
  });

  it('holds a static balance without a monthly payment', () => {
    const traj = loanTrajectory({ ...zeroRate, monthlyPayment: 0 }, day(2025, 1, 1));
    expect(traj.past).toEqual([
      { t: START, value: 120_000 },
      { t: day(2025, 1, 1), value: 120_000 },
    ]);
    expect(traj.future).toHaveLength(0);
    expect(traj.payoffDay).toBeNull();
    expect(traj.end).toBe('noPayment');
  });
});

describe('stepValueAt / clipSeries', () => {
  const pts = [
    { t: 10, value: 100 },
    { t: 20, value: 80 },
    { t: 30, value: 0 },
  ];
  it('reads the last value on or before t and 0 before the first', () => {
    expect(stepValueAt(pts, 5)).toBe(0);
    expect(stepValueAt(pts, 10)).toBe(100);
    expect(stepValueAt(pts, 25)).toBe(80);
    expect(stepValueAt(pts, 99)).toBe(0);
  });
  it('clips to the window with readings on both edges', () => {
    expect(clipSeries(pts, 0, 25)).toEqual([
      { t: 10, value: 100 },
      { t: 20, value: 80 },
      { t: 25, value: 80 },
    ]);
    expect(clipSeries(pts, 15, 100)).toEqual([
      { t: 15, value: 100 },
      { t: 20, value: 80 },
      { t: 30, value: 0 },
      { t: 100, value: 0 },
    ]);
  });
});

describe('debtSeries', () => {
  it('sums loans on a shared grid, counting a loan as 0 before it starts', () => {
    const today = day(2024, 3, 15);
    const a = loanTrajectory(zeroRate, today);
    const b = loanTrajectory(
      { principal: 50_000, annualRatePct: 0, monthlyPayment: 25_000, startDay: day(2024, 3, 1) },
      today
    );
    const sum = debtSeries(
      [
        { trajectory: a, convert: (amount: number) => amount },
        { trajectory: b, convert: (amount: number) => amount * 2 },
      ],
      today
    );
    const at = (t: number) => sum.find((p) => p.t === t)?.value;
    expect(at(START)).toBe(120_000);
    expect(at(day(2024, 3, 1))).toBe(100_000 + 2 * 50_000);
    expect(at(today)).toBe(100_000 + 100_000);
    expect(at(day(2024, 5, 1))).toBe(80_000 + 0);
    expect(sum[sum.length - 1].value).toBe(0);
  });

  it('converts every reading at its own day and leaves out the days without a rate', () => {
    const today = day(2024, 3, 15);
    const a = loanTrajectory(zeroRate, today);
    const noRateOn = day(2024, 3, 1);
    const sum = debtSeries(
      [
        {
          trajectory: a,
          // 2 per unit before March, 3 from March on, no rate on 1 March
          convert: (amount: number, t: number) =>
            t === noRateOn ? null : amount * (t < noRateOn ? 2 : 3),
        },
      ],
      today
    );
    const at = (t: number) => sum.find((p) => p.t === t)?.value;
    expect(at(START)).toBe(240_000);
    expect(sum.some((p) => p.t === noRateOn)).toBe(false);
    expect(at(today)).toBe(300_000);
    // A zero balance needs no rate
    expect(sum[sum.length - 1].value).toBe(0);
  });

  describe('the day a later loan starts', () => {
    const today = day(2024, 6, 15);
    /** A debtSeries input in its own currency, the terms over the zero-rate loan above. */
    const item = (terms: Partial<LoanTerms>) => ({
      trajectory: loanTrajectory({ ...zeroRate, ...terms }, today),
      convert: (amount: number) => amount,
    });
    const laterStart = day(2024, 3, 20);
    const later = { principal: 60_000, monthlyPayment: 5_000, startDay: laterStart };

    it('steps up vertically instead of ramping from the previous reading', () => {
      // The first loan is due on the 1st, the later one on the 20th: without a reading the
      // day before, the sum would climb from the 1 March reading to 20 March.
      const sum = debtSeries([item({}), item(later)], today);
      const at = (t: number) => sum.find((p) => p.t === t)?.value;
      // February and March are paid on the first loan; the later one does not exist yet...
      expect(at(laterStart - DAY)).toBe(100_000);
      // ...and counts in full the next day, with no reading in between
      expect(at(laterStart)).toBe(100_000 + 60_000);
      const i = sum.findIndex((p) => p.t === laterStart);
      expect(sum[i - 1].t).toBe(laterStart - DAY);
    });

    it('steps for a loan that starts after today too', () => {
      const futureStart = day(2024, 9, 20);
      const sum = debtSeries(
        [item({}), item({ principal: 60_000, monthlyPayment: 5_000, startDay: futureStart })],
        today
      );
      const at = (t: number) => sum.find((p) => p.t === t)?.value;
      // Eight payments (February to September) leave 40 000 on the first loan
      expect(at(futureStart - DAY)).toBe(40_000);
      expect(at(futureStart)).toBe(40_000 + 60_000);
    });

    it('adds no step before the earliest loan, where the series begins', () => {
      const sum = debtSeries([item({}), item(later)], today);
      expect(sum[0]).toEqual({ t: START, value: 120_000 });
      // Loans that share the earliest day do not step either
      const together = debtSeries([item({}), item({ principal: 60_000 })], today);
      expect(together[0]).toEqual({ t: START, value: 180_000 });
      expect(together.some((p) => p.t === START - DAY)).toBe(false);
    });

    it('keeps the step readings when a long grid is thinned', () => {
      // Three 30-year loans due on three different days: about 1 100 distinct readings,
      // far above the 480-point budget, so most of the grid is dropped on a stride.
      const thirtyYears = { principal: 3_600_000, monthlyPayment: 10_000 };
      const secondStart = day(2024, 2, 10);
      const sum = debtSeries(
        [
          item(thirtyYears),
          item({ ...thirtyYears, startDay: secondStart }),
          item({ ...thirtyYears, startDay: laterStart }),
        ],
        today
      );
      expect(sum.length).toBeLessThan(600);
      const at = (t: number) => sum.find((p) => p.t === t)?.value;
      // 1 Feb is paid once (3 590 000 left); the second loan does not exist yet
      expect(at(secondStart - DAY)).toBe(3_590_000);
      expect(at(secondStart)).toBe(3_590_000 + 3_600_000);
      // 1 Mar is paid twice (3 580 000) and 10 Mar once (3 590 000); the third does not exist yet
      expect(at(laterStart - DAY)).toBe(3_580_000 + 3_590_000);
      expect(at(laterStart)).toBe(3_580_000 + 3_590_000 + 3_600_000);
    });
  });
});

describe('extraPaymentWhatIf', () => {
  const terms: LoanTerms = {
    principal: 1_000_000,
    annualRatePct: 6,
    monthlyPayment: 20_000,
    startDay: START,
  };
  const today = day(2026, 1, 1);

  const whatIf = (
    amount: number,
    mode: 'term' | 'payment',
    t: LoanTerms = terms,
    events: LedgerEvent[] = []
  ) => {
    const ledger = ledgerOf(t, events);
    return extraPaymentWhatIf(
      planFrom(ledger, today),
      loanBalanceAt(ledger, today).balance,
      today,
      amount,
      mode
    );
  };

  it('shortens the term at the same payment and saves interest', () => {
    const result = whatIf(100_000, 'term');
    expect(result).not.toBeNull();
    expect(result!.payment).toBe(20_000);
    expect(result!.monthsAfter).toBeLessThan(result!.monthsBefore);
    expect(result!.interestAfter).toBeLessThan(result!.interestBefore);
    expect(result!.payoffDay).not.toBeNull();
  });

  it('keeps the term and lowers the payment', () => {
    const result = whatIf(100_000, 'payment');
    expect(result!.payment).toBeLessThan(20_000);
    expect(result!.monthsAfter).toBe(result!.monthsBefore);
  });

  it('is null without a payment and clamps the amount to the balance', () => {
    expect(whatIf(1, 'term', { ...terms, monthlyPayment: 0 })).toBeNull();
    const all = whatIf(10_000_000, 'term');
    expect(all!.balanceAfter).toBe(0);
    expect(all!.monthsAfter).toBe(0);
  });
});

describe('extraPaymentWhatIf with a manual balance', () => {
  it('works on the plan ahead from the manual balance', () => {
    const terms: LoanTerms = {
      principal: 1_000_000,
      annualRatePct: 6,
      monthlyPayment: 20_000,
      startDay: START,
    };
    const today = day(2026, 1, 1);
    const ledger = ledgerOf(terms, [manual(day(2025, 6, 15), 600_000)]);
    const balance = loanBalanceAt(ledger, today).balance;
    expect(balance).toBe(600_000);
    const result = extraPaymentWhatIf(planFrom(ledger, today), balance, today, 100_000, 'term');
    expect(result!.balanceAfter).toBe(500_000);
    expect(result!.monthsAfter).toBeLessThan(result!.monthsBefore);
  });
});

describe('planPayoffDay', () => {
  it('is the anchor day for an empty anchor and the last payment otherwise', () => {
    const plan: LoanTerms = { ...zeroRate, anchorAmount: 30_000, anchorDay: day(2024, 6, 1) };
    expect(planPayoffDay(plan, day(2024, 6, 15))).toBe(day(2024, 9, 1));
    expect(planPayoffDay({ ...plan, anchorAmount: 0 }, day(2024, 6, 15))).toBe(day(2024, 6, 1));
    expect(planPayoffDay({ ...plan, monthlyPayment: 0 }, day(2024, 6, 15))).toBeNull();
  });
});

describe('annuityPayment', () => {
  it('matches the textbook annuity and the linear case', () => {
    expect(annuityPayment(120_000, 0, 12)).toBe(10_000);
    expect(annuityPayment(1_000_000, 6, 120)).toBeCloseTo(11_102.05, 1);
    expect(annuityPayment(0, 6, 120)).toBe(0);
  });
});

describe('whatIfStep / suggestedExtraPayment', () => {
  it('steps by 1 % of the balance rounded down to a power of ten', () => {
    expect(whatIfStep(3_000_000)).toBe(10_000);
    expect(whatIfStep(120_000)).toBe(1_000);
    expect(whatIfStep(100_000)).toBe(1_000);
    expect(whatIfStep(9_999)).toBe(10);
  });

  it('never steps below one unit', () => {
    expect(whatIfStep(50)).toBe(1);
    expect(whatIfStep(0)).toBe(1);
  });

  it('suggests 5 % of the balance in whole steps, at least one step', () => {
    expect(suggestedExtraPayment(3_000_000)).toBe(150_000);
    expect(suggestedExtraPayment(120_000)).toBe(6_000);
    expect(suggestedExtraPayment(0)).toBe(1);
  });
});
