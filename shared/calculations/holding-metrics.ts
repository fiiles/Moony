/**
 * Pure calculation functions for individual holdings.
 * These are stateless functions with no side effects.
 */

// ============================================================
// Types
// ============================================================

/**
 * Input for holding-level calculations.
 * All numeric values should be pre-parsed (no strings).
 */
export interface HoldingInput {
  quantity: number;
  averagePrice: number;
  currentPrice: number;
}

/**
 * Calculated metrics for a single holding.
 */
export interface HoldingMetrics {
  totalCost: number;
  marketValue: number;
  gainLoss: number;
  gainLossPercent: number;
}

// ============================================================
// Pure Calculation Functions
// ============================================================

/**
 * Calculate market value: quantity * currentPrice
 */
export function calculateMarketValue(quantity: number, price: number): number {
  return quantity * price;
}

/**
 * Calculate total cost: quantity * averagePrice
 */
export function calculateTotalCost(quantity: number, averagePrice: number): number {
  return quantity * averagePrice;
}

/**
 * Calculate gain/loss: marketValue - totalCost
 */
export function calculateGainLoss(marketValue: number, totalCost: number): number {
  return marketValue - totalCost;
}

/**
 * Calculate gain/loss percentage: (gainLoss / totalCost) * 100
 * Returns 0 if totalCost is 0 to avoid division by zero.
 */
export function calculateGainLossPercent(gainLoss: number, totalCost: number): number {
  return totalCost > 0 ? (gainLoss / totalCost) * 100 : 0;
}

/**
 * Calculate all holding metrics from input values.
 * This is the main entry point for single-holding calculations.
 */
export function calculateHoldingMetrics(input: HoldingInput): HoldingMetrics {
  const totalCost = calculateTotalCost(input.quantity, input.averagePrice);
  const marketValue = calculateMarketValue(input.quantity, input.currentPrice);
  const gainLoss = calculateGainLoss(marketValue, totalCost);
  const gainLossPercent = calculateGainLossPercent(gainLoss, totalCost);

  return {
    totalCost,
    marketValue,
    gainLoss,
    gainLossPercent,
  };
}
