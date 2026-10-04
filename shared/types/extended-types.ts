/**
 * Extended type definitions for API responses and enriched data.
 * These interfaces augment the base schema types with computed/fetched fields.
 */

import type {
  StockInvestment,
  CryptoInvestment,
  RealEstate,
  Loan,
  InsurancePolicy,
  InvestmentTransaction,
  StockTag,
} from '../schema';

// ============================================================
// Investment Types (Enriched with price data)
// ============================================================

/**
 * Stock investment enriched with current price data from API.
 * Used when the API returns investments with their latest prices.
 *
 * Note: averagePriceCurrency is inherited from StockInvestment base type
 * and represents the currency of the stored average purchase price.
 */
export interface StockInvestmentWithPrice extends StockInvestment {
  currentPrice: number;
  /** Original price in its source currency (before conversion) */
  originalPrice?: number;
  fetchedAt?: Date | string;
  isManualPrice?: boolean;
  dividendYield?: number;
  /** Original dividend amount before conversion */
  originalDividendYield?: number;
  dividendCurrency?: string;
  isManualDividend?: boolean;
  /** Currency of the current market price (e.g., USD, EUR) */
  currency?: string;
}

/**
 * Crypto investment enriched with current price data.
 */
export interface CryptoInvestmentWithPrice extends CryptoInvestment {
  currentPrice: number;
  /** Original price in its source currency (before conversion) */
  originalPrice?: number;
  /** Currency of the average price (native currency from first transaction) */
  averagePriceCurrency?: string;
  /** Currency of the current market price (e.g., USD, EUR) */
  currency?: string;
  fetchedAt?: Date | string;
  isManualPrice?: boolean;
}

/**
 * Compound response for investment details (reduces IPC calls)
 */
export interface InvestmentWithDetails {
  investment: StockInvestmentWithPrice;
  transactions: InvestmentTransaction[];
  tags: StockTag[];
}

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
