import { describe, expect, it } from 'vitest';
import type { LoanTerms } from '@shared/calculations/loan-amortization';
import {
  annuityPayment,
  clipSeries,
  debtSeries,
  extraPaymentWhatIf,
  loanTrajectory,
  stepValueAt,
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

  it('steps to a manual anchor on its day and keeps the theoretical path before it', () => {
    const anchorDay = day(2024, 4, 1);
    const traj = loanTrajectory({ ...zeroRate, anchorAmount: 50_000, anchorDay }, day(2024, 4, 20));
    const before = traj.past.find((p) => p.t === anchorDay - DAY);
    const at = traj.past.find((p) => p.t === anchorDay);
    // Feb and Mar payments were made on the theoretical path: 100 000 left
    expect(before?.value).toBe(100_000);
    expect(at?.value).toBe(50_000);
    expect(traj.balanceToday).toBe(50_000);
    expect(traj.payoffDay).toBe(day(2024, 9, 1));
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
        { trajectory: a, rate: 1 },
        { trajectory: b, rate: 2 },
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
});

describe('extraPaymentWhatIf', () => {
  const terms: LoanTerms = {
    principal: 1_000_000,
    annualRatePct: 6,
    monthlyPayment: 20_000,
    startDay: START,
  };
  const today = day(2026, 1, 1);

  it('shortens the term at the same payment and saves interest', () => {
    const result = extraPaymentWhatIf(terms, today, 100_000, 'term');
    expect(result).not.toBeNull();
    expect(result!.payment).toBe(20_000);
    expect(result!.monthsAfter).toBeLessThan(result!.monthsBefore);
    expect(result!.interestAfter).toBeLessThan(result!.interestBefore);
    expect(result!.payoffDay).not.toBeNull();
  });

  it('keeps the term and lowers the payment', () => {
    const result = extraPaymentWhatIf(terms, today, 100_000, 'payment');
    expect(result!.payment).toBeLessThan(20_000);
    expect(result!.monthsAfter).toBe(result!.monthsBefore);
  });

  it('is null without a payment and clamps the amount to the balance', () => {
    expect(extraPaymentWhatIf({ ...terms, monthlyPayment: 0 }, today, 1, 'term')).toBeNull();
    const all = extraPaymentWhatIf(terms, today, 10_000_000, 'term');
    expect(all!.balanceAfter).toBe(0);
    expect(all!.monthsAfter).toBe(0);
  });
});

describe('annuityPayment', () => {
  it('matches the textbook annuity and the linear case', () => {
    expect(annuityPayment(120_000, 0, 12)).toBe(10_000);
    expect(annuityPayment(1_000_000, 6, 120)).toBeCloseTo(11_102.05, 1);
    expect(annuityPayment(0, 6, 120)).toBe(0);
  });
});
