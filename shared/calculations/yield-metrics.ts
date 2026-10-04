/**
 * Pure calculation functions for yield metrics.
 * Used by "Other Assets" and similar yield-generating investments.
 */

// ============================================================
// Types
// ============================================================

export type YieldType = 'none' | 'fixed' | 'percent_purchase' | 'percent_market';

/**
 * Input for yield calculations.
 * All numeric values should be pre-parsed (no strings).
 */
export interface YieldInput {
  yieldType: YieldType;
  yieldValue: number;
  quantity: number;
  averagePurchasePrice: number;
  marketPrice: number;
}

// ============================================================
// Pure Calculation Functions
// ============================================================

/**
 * Calculate annual yield based on yield type and input values.
 *
 * @param input - The yield calculation input
 * @returns Annual yield amount in the asset's currency
 *
 * Yield types:
 * - 'none': No yield, returns 0
 * - 'fixed': Fixed annual amount (yieldValue is the amount)
 * - 'percent_purchase': Percentage of purchase cost (yieldValue is percentage, e.g., 5 for 5%)
 * - 'percent_market': Percentage of current market value (yieldValue is percentage)
 */
export function calculateAnnualYield(input: YieldInput): number {
  const { yieldType, yieldValue, quantity, averagePurchasePrice, marketPrice } = input;

  switch (yieldType) {
    case 'fixed':
      return yieldValue;
    case 'percent_purchase': {
      const totalCost = quantity * averagePurchasePrice;
      return (yieldValue / 100) * totalCost;
    }
    case 'percent_market': {
      const marketValue = quantity * marketPrice;
      return (yieldValue / 100) * marketValue;
    }
    case 'none':
    default:
      return 0;
  }
}

/**
 * Calculate yield percentage relative to purchase cost.
 * Useful for comparing yields across different investments.
 */
export function calculateYieldPercentOnCost(annualYield: number, totalCost: number): number {
  return totalCost > 0 ? (annualYield / totalCost) * 100 : 0;
}

/**
 * Calculate yield percentage relative to market value.
 */
export function calculateYieldPercentOnMarket(annualYield: number, marketValue: number): number {
  return marketValue > 0 ? (annualYield / marketValue) * 100 : 0;
}
