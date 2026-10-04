/**
 * Pure calculation functions for percentage change metrics.
 * Used by Dashboard and other components showing historical changes.
 */

// ============================================================
// Pure Calculation Functions
// ============================================================

/**
 * Calculate percentage change between two values.
 * Handles edge cases like zero previous value.
 *
 * @param current - Current value
 * @param previous - Previous value (baseline)
 * @returns Percentage change (e.g., 20 for +20%, -15 for -15%)
 */
export function calculatePercentageChange(current: number, previous: number): number {
  if (previous === 0) return 0;
  return ((current - previous) / Math.abs(previous)) * 100;
}

/**
 * Calculate absolute change between two values.
 */
export function calculateAbsoluteChange(current: number, previous: number): number {
  return current - previous;
}

/**
 * Calculate net worth from assets and liabilities.
 */
export function calculateNetWorth(totalAssets: number, totalLiabilities: number): number {
  return totalAssets - totalLiabilities;
}

/**
 * Calculate total assets from individual components.
 * Useful for ensuring consistent calculation across the app.
 */
export interface AssetComponents {
  savings: number;
  investments: number;
  crypto: number;
  bonds: number;
  realEstatePersonal: number;
  realEstateInvestment: number;
  excludePersonalRealEstate?: boolean;
}

export function calculateTotalAssets(components: AssetComponents): number {
  const {
    savings,
    investments,
    crypto,
    bonds,
    realEstatePersonal,
    realEstateInvestment,
    excludePersonalRealEstate = false,
  } = components;

  return (
    savings +
    investments +
    crypto +
    bonds +
    (excludePersonalRealEstate ? 0 : realEstatePersonal) +
    realEstateInvestment
  );
}

/**
 * Calculate allocation percentage for a category.
 */
export function calculateAllocationPercentage(categoryValue: number, totalAssets: number): number {
  return totalAssets > 0 ? Math.round((categoryValue / totalAssets) * 100) : 0;
}
