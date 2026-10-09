/**
 * Stock and crypto positions as the list, detail and trade dialogs use them: the backend's
 * response fields (ADR 0011) parsed into numbers. No holding maths happens here — value, cost
 * basis and gains are the backend's, in the response's report currency, and stay `null` when
 * they could not be converted.
 */

import type { EnrichedCryptoInvestment, EnrichedStockInvestment } from '@shared/schema';

/** A converted wire amount (decimal TEXT) as a number; `null` (not convertible) stays `null`. */
export function amountOf(value: string | null | undefined): number | null {
  if (value === null || value === undefined) return null;
  const n = Number(value);
  return Number.isFinite(n) ? n : null;
}

/** Cost per unit in the report currency; `null` without a known cost or an open quantity. */
export function costPerUnit(costBasis: number | null, quantity: number): number | null {
  return costBasis !== null && quantity > 0 ? costBasis / quantity : null;
}

/** What every position row shares, stock or crypto. */
interface PositionRowBase {
  id: string;
  ticker: string;
  quantity: number;
  /** Native weighted average cost and its currency. */
  originalAvgCost: number;
  originalAvgCostCurrency: string;
  /** Native current price (0 when unpriced) and its currency. */
  originalCurrentPrice: number;
  originalCurrency: string;
  fetchedAt: number | null;
  isManualPrice: boolean;
  /** Report-currency figures from the backend; `null` when not convertible or unknown. */
  marketValue: number | null;
  totalCost: number | null;
  gainLoss: number | null;
  gainLossPercent: number | null;
  realizedGain: number | null;
}

function baseRow(inv: EnrichedStockInvestment | EnrichedCryptoInvestment): PositionRowBase {
  return {
    id: inv.id,
    ticker: inv.ticker,
    quantity: Number(inv.quantity) || 0,
    originalAvgCost: Number(inv.averagePrice) || 0,
    originalAvgCostCurrency: inv.averagePriceCurrency,
    originalCurrentPrice: Number(inv.price ?? 0) || 0,
    originalCurrency: inv.priceCurrency ?? inv.currency,
    fetchedAt: inv.fetchedAt,
    isManualPrice: inv.isManualPrice,
    marketValue: amountOf(inv.value),
    totalCost: amountOf(inv.costBasis),
    gainLoss: amountOf(inv.unrealizedGain),
    gainLossPercent: inv.unrealizedGainPercent,
    realizedGain: amountOf(inv.realizedGain),
  };
}

/** A stock position row. */
export interface StockRow extends PositionRowBase {
  companyName: string;
  isManualDividend: boolean;
  /** Yearly dividend per share in its own currency, 0 without dividend data. */
  originalDividendYield: number;
  dividendCurrency: string | null;
  /** Yearly dividend of the position in the report currency. */
  yearlyDividend: number | null;
}

export function toStockRow(inv: EnrichedStockInvestment): StockRow {
  return {
    ...baseRow(inv),
    companyName: inv.companyName,
    isManualDividend: inv.isManualDividend,
    originalDividendYield: inv.originalDividendYield,
    dividendCurrency: inv.dividendCurrency,
    yearlyDividend: amountOf(inv.yearlyDividend),
  };
}

/** A crypto position row. */
export interface CryptoRow extends PositionRowBase {
  name: string;
  coingeckoId: string;
}

export function toCryptoRow(inv: EnrichedCryptoInvestment): CryptoRow {
  return { ...baseRow(inv), name: inv.name || inv.ticker, coingeckoId: inv.coingeckoId };
}

/** The open position with the highest known return in percent, if any. */
export function topPerformer<T extends PositionRowBase>(rows: readonly T[]): T | null {
  return rows.reduce<T | null>((best, row) => {
    if (row.quantity <= 0 || row.gainLossPercent === null) return best;
    return best === null || row.gainLossPercent > (best.gainLossPercent ?? -Infinity) ? row : best;
  }, null);
}

/** Return in percent of the cost; 0 without a cost basis (as the backend's percent). */
export function percentOfCost(gain: number, cost: number): number {
  return cost > 0 ? (gain / cost) * 100 : 0;
}

/** Ascending order of possibly unknown numbers; unknown (`null`) sorts below every number. */
export function compareKnown(a: number | null, b: number | null): number {
  if (a === b) return 0;
  if (a === null) return -1;
  if (b === null) return 1;
  return a - b;
}
