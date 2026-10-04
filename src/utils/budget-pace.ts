import type { PeriodTimeframe } from '@/utils/period';

const DAY = 86_400;

export interface PeriodPace {
  /** Days in the period. */
  days: number;
  /** Days of the period already behind us, today included (0 for a future period). */
  elapsedDays: number;
  /** Whole days still ahead, today excluded. */
  remainingDays: number;
  /** 0–1 share of the period behind us. */
  elapsed: number;
  /** The period contains today. */
  isCurrent: boolean;
  /** The period lies entirely in the past. */
  isPast: boolean;
}

/**
 * Where "today" sits inside a UTC period (design system §9, prototype
 * `budgets.html`: "6 % měsíce za vámi · na 29 dní"). Booking dates are UTC
 * calendar days (ADR 0008), so the day of `now` is taken in UTC too.
 */
export function periodPace(start: number, end: number, now: Date = new Date()): PeriodPace {
  const days = Math.max(1, Math.round((end + 1 - start) / DAY));
  const today = Math.floor(now.getTime() / 1000 / DAY) * DAY;
  if (today < start) {
    return {
      days,
      elapsedDays: 0,
      remainingDays: days,
      elapsed: 0,
      isCurrent: false,
      isPast: false,
    };
  }
  if (today > end) {
    return {
      days,
      elapsedDays: days,
      remainingDays: 0,
      elapsed: 1,
      isCurrent: false,
      isPast: true,
    };
  }
  const elapsedDays = Math.round((today - start) / DAY) + 1;
  return {
    days,
    elapsedDays,
    remainingDays: days - elapsedDays,
    elapsed: elapsedDays / days,
    isCurrent: true,
    isPast: false,
  };
}

export type BudgetState = 'over' | 'near' | 'ok';

/** Over the limit, within 15 % of it, or fine (prototype thresholds: >100 %, ≥85 %). */
export function budgetState(spent: number, limit: number): BudgetState {
  if (limit <= 0) return 'ok';
  const ratio = spent / limit;
  if (ratio > 1) return 'over';
  if (ratio >= 0.85) return 'near';
  return 'ok';
}

/** Monthly limits are stored once and scale to the quarter and the year. */
export function budgetMultiplier(timeframe: PeriodTimeframe): number {
  return timeframe === 'yearly' ? 12 : timeframe === 'quarterly' ? 3 : 1;
}

export type PaceVerdict = 'ahead' | 'behind' | 'onPace';

/**
 * Spending share against the elapsed share of the period, in percentage
 * points; more than 10 points either way is worth a sentence.
 */
export function paceVerdict(
  spentShare: number,
  elapsed: number
): { verdict: PaceVerdict; points: number } {
  const points = Math.round((spentShare - elapsed) * 100);
  if (points > 10) return { verdict: 'ahead', points };
  if (points < -10) return { verdict: 'behind', points: -points };
  return { verdict: 'onPace', points: Math.abs(points) };
}
