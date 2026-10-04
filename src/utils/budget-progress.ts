import type { PeriodTimeframe } from '@/utils/period';

/** Budgets are stored per month: how many of them make up the viewed period. */
export function budgetMultiplier(timeframe: PeriodTimeframe): number {
  return timeframe === 'yearly' ? 12 : timeframe === 'quarterly' ? 3 : 1;
}

export interface BudgetUsage {
  /** The monthly limit scaled to the period, in CZK. */
  limitCzk: number;
  /** Spent / limit as a whole percentage (can exceed 100). */
  percent: number;
  /** Width of the progress bar, 0-100. */
  fill: number;
  /** Strictly more than the limit has been spent. */
  over: boolean;
}

/**
 * How much of a category's budget is used in the viewed period. Spent and limit are
 * both in CZK; the monthly limit is scaled to the period so a quarter's spending is not held
 * against a single month's budget.
 */
export function budgetUsage(
  spentCzk: number,
  monthlyLimitCzk: number,
  timeframe: PeriodTimeframe
): BudgetUsage {
  const limitCzk = monthlyLimitCzk * budgetMultiplier(timeframe);
  if (!(limitCzk > 0)) return { limitCzk: 0, percent: 0, fill: 0, over: false };
  const ratio = spentCzk / limitCzk;
  return {
    limitCzk,
    percent: Math.round(ratio * 100),
    fill: Math.min(ratio * 100, 100),
    over: spentCzk > limitCzk,
  };
}
