/**
 * Pure calculation functions for crypto portfolio metrics.
 * Extends the base holding-metrics with crypto-specific functionality.
 */

import type { HoldingMetrics } from './holding-metrics';
import { calculateHoldingMetrics, calculatePortfolioTotals } from './index';

// ============================================================
// Types
// ============================================================

/**
 * Input for crypto holding calculations.
 * Similar to HoldingInput but uses crypto-specific field names for clarity.
 */
export interface CryptoHoldingInput {
  quantity: number;
  averagePrice: number;
  currentPrice: number;
}

/**
 * Extended crypto holding data with identification and metadata.
 */
export interface CryptoHoldingData extends HoldingMetrics {
  id: string;
  ticker: string;
  name: string;
  coingeckoId?: string;
  quantity: number;
  avgCost: number;
  currentPrice: number;
  /** Original price in its source currency (before conversion) */
  originalPrice?: number;
  /** Currency of the original price (e.g., USD, EUR) */
  currency?: string;
  fetchedAt?: Date | null;
  isManualPrice?: boolean;
}

/**
 * Aggregated metrics for an entire crypto portfolio.
 */
export interface CryptoPortfolioMetrics {
  totalValue: number;
  totalCost: number;
  overallGainLoss: number;
  overallGainLossPercent: number;
  largestHolding: { ticker: string; value: number } | null;
}

// ============================================================
// Pure Calculation Functions
// ============================================================

/**
 * Calculate metrics for a single crypto holding.
 * Wrapper around the base calculateHoldingMetrics for type consistency.
 */
export function calculateCryptoHoldingMetrics(input: CryptoHoldingInput): HoldingMetrics {
  return calculateHoldingMetrics({
    quantity: input.quantity,
    averagePrice: input.averagePrice,
    currentPrice: input.currentPrice,
  });
}

/**
 * Find the largest holding by market value.
 */
export function findLargestHolding(
  holdings: CryptoHoldingData[]
): { ticker: string; value: number } | null {
  if (holdings.length === 0) return null;

  return holdings.reduce(
    (largest, current) => {
      if (!largest || current.marketValue > largest.value) {
        return { ticker: current.ticker, value: current.marketValue };
      }
      return largest;
    },
    null as { ticker: string; value: number } | null
  );
}

/**
 * Calculate aggregated portfolio metrics for crypto holdings.
 * Includes crypto-specific metrics like largest holding.
 */
export function calculateCryptoPortfolioMetrics(
  holdings: CryptoHoldingData[]
): CryptoPortfolioMetrics {
  const totals = calculatePortfolioTotals(holdings);

  return {
    totalValue: totals.totalValue,
    totalCost: totals.totalCost,
    overallGainLoss: totals.overallGainLoss,
    overallGainLossPercent: totals.overallGainLossPercent,
    largestHolding: findLargestHolding(holdings),
  };
}

/**
 * Map raw crypto investment data to CryptoHoldingData.
 * Use this instead of inline mapping in components.
 */
export function mapCryptoInvestmentToHolding(
  id: string,
  ticker: string,
  name: string,
  quantity: number,
  averagePrice: number,
  currentPrice: number,
  fetchedAt?: Date | string | null,
  isManualPrice?: boolean,
  coingeckoId?: string,
  originalPrice?: number,
  currency?: string
): CryptoHoldingData {
  const metrics = calculateCryptoHoldingMetrics({ quantity, averagePrice, currentPrice });

  // Convert string dates to Date objects
  let parsedFetchedAt: Date | null | undefined;
  if (fetchedAt === null) {
    parsedFetchedAt = null;
  } else if (typeof fetchedAt === 'string') {
    parsedFetchedAt = new Date(fetchedAt);
  } else {
    parsedFetchedAt = fetchedAt;
  }

  return {
    id,
    ticker,
    name,
    coingeckoId,
    quantity,
    avgCost: averagePrice,
    currentPrice,
    originalPrice,
    currency,
    totalCost: metrics.totalCost,
    marketValue: metrics.marketValue,
    gainLoss: metrics.gainLoss,
    gainLossPercent: metrics.gainLossPercent,
    fetchedAt: parsedFetchedAt,
    isManualPrice,
  };
}
