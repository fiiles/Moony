/**
 * Extended type definitions for API responses and enriched data.
 * These interfaces augment the base schema types with computed/fetched fields.
 */

import type { RealEstate, Loan, InsurancePolicy } from '../schema';

// ============================================================
// Real Estate Types
// ============================================================

/**
 * Recurring cost definition for real estate properties.
 */
export interface RecurringCost {
  name: string;
  amount: number;
  frequency: 'monthly' | 'quarterly' | 'yearly' | string;
  currency?: string;
}

/**
 * Real estate with properly typed recurring costs.
 */
export interface RealEstateWithCosts extends Omit<RealEstate, 'recurringCosts'> {
  recurringCosts: RecurringCost[];
}

// ============================================================
// Financial Entity Types (with full currency support)
// ============================================================

/**
 * Loan with currency field guaranteed to be present.
 * Note: This is an alias since the schema already has currency as required.
 */
export type LoanWithCurrency = Loan;

/**
 * Insurance policy with currency fields guaranteed.
 * Note: This is an alias since the schema already has these fields.
 */
export type InsurancePolicyWithCurrency = InsurancePolicy;

// ============================================================
// Utility Types
// ============================================================

/**
 * Makes specific keys required in a type.
 */
export type RequireKeys<T, K extends keyof T> = T & Required<Pick<T, K>>;

/**
 * Type guard to check if an investment has price data.
 */
export function hasCurrentPrice<T extends { currentPrice?: number }>(
  investment: T
): investment is T & { currentPrice: number } {
  return typeof investment.currentPrice === 'number';
}
