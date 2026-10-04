/**
 * Pure calculation functions for portfolio-level aggregations.
 * These are stateless functions with no side effects.
 */

import type { HoldingMetrics } from './holding-metrics';

// ============================================================
// Types
// ============================================================

/**
 * Aggregated portfolio totals.
 */
export interface PortfolioTotals {
  totalValue: number;
  totalCost: number;
  overallGainLoss: number;
  overallGainLossPercent: number;
}

// ============================================================
// Pure Calculation Functions
// ============================================================

/**
 * Sum a specific numeric property across all holdings.
 */
export function sumHoldingValues(holdings: HoldingMetrics[], key: keyof HoldingMetrics): number {
  return holdings.reduce((sum, h) => sum + h[key], 0);
}

/**
 * Calculate aggregated portfolio totals from an array of holdings.
 * Returns zeros for empty arrays.
 */
export function calculatePortfolioTotals(holdings: HoldingMetrics[]): PortfolioTotals {
  if (holdings.length === 0) {
    return {
      totalValue: 0,
      totalCost: 0,
      overallGainLoss: 0,
      overallGainLossPercent: 0,
    };
  }

  const totalValue = sumHoldingValues(holdings, 'marketValue');
  const totalCost = sumHoldingValues(holdings, 'totalCost');
  const overallGainLoss = totalValue - totalCost;
  const overallGainLossPercent = totalCost > 0 ? (overallGainLoss / totalCost) * 100 : 0;

  return {
    totalValue,
    totalCost,
    overallGainLoss,
    overallGainLossPercent,
  };
}

/**
 * Find the top performer by gain/loss percentage.
 * Returns null for empty arrays.
 */
export function findTopPerformer<T extends HoldingMetrics>(holdings: T[]): T | null {
  if (holdings.length === 0) return null;

  return holdings.reduce(
    (top, current) => {
      return current.gainLossPercent > (top?.gainLossPercent || -Infinity) ? current : top;
    },
    null as T | null
  );
}
