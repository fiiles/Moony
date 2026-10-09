/**
 * Pure calculation function for realized gains/losses.
 * Uses weighted average cost (WAC) method per ticker.
 */

import type { CurrencyCode } from '../currencies';

/**
 * Chronological order for replaying transactions. Day-granular dates give
 * every same-day transaction the same timestamp, and the API lists them
 * newest-first without a tiebreak, so a plain stable sort can put a sell in
 * front of the buys it depends on. Within one day buys are
 * processed before sells; `createdAt` (then input order) breaks the rest.
 */
export function compareChronologically(
  a: { type: string; transactionDate: number; createdAt?: number },
  b: { type: string; transactionDate: number; createdAt?: number }
): number {
  if (a.transactionDate !== b.transactionDate) return a.transactionDate - b.transactionDate;
  const aSell = a.type === 'sell' ? 1 : 0;
  const bSell = b.type === 'sell' ? 1 : 0;
  if (aSell !== bSell) return aSell - bSell;
  if (a.createdAt !== undefined && b.createdAt !== undefined) return a.createdAt - b.createdAt;
  return 0;
}

/**
 * Convert `amount` from one currency to another at the rates of the given
 * day (unix seconds). An implementation without a rate for that day returns
 * NaN, which makes the realized result unknown rather than wrong.
 */
export type DatedConvertFn = (
  amount: number,
  from: CurrencyCode,
  to: CurrencyCode,
  dateTs: number
) => number;

export interface RealizedGainTransaction {
  type: string; // 'buy' | 'sell'
  ticker: string;
  quantity: string;
  pricePerUnit: string;
  currency: string;
  transactionDate: number;
  createdAt?: number;
}

/**
 * Calculate total realized gains/losses from a list of transactions.
 * Uses weighted average cost (WAC) method per ticker.
 * Returns the result in targetCurrency, converting every transaction at the
 * exchange rate of its own day (ADR 0001: converting old values at today's
 * rate is wrong) — buy legs carry their buy-day rate in the cost basis, sell
 * proceeds convert at the sell-day rate.
 */
export function calculateRealizedGains(
  transactions: RealizedGainTransaction[],
  convertAt: DatedConvertFn,
  targetCurrency: CurrencyCode
): number {
  const byTicker = new Map<string, RealizedGainTransaction[]>();
  for (const tx of transactions) {
    const group = byTicker.get(tx.ticker) ?? [];
    group.push(tx);
    byTicker.set(tx.ticker, group);
  }

  let totalRealizedGain = 0;

  for (const txs of byTicker.values()) {
    txs.sort(compareChronologically);

    let runningQty = 0;
    let runningCost = 0;

    for (const tx of txs) {
      const qty = parseFloat(tx.quantity) || 0;
      const priceRaw = parseFloat(tx.pricePerUnit) || 0;
      const price = convertAt(
        priceRaw,
        tx.currency as CurrencyCode,
        targetCurrency,
        tx.transactionDate
      );

      if (tx.type === 'buy') {
        runningCost += qty * price;
        runningQty += qty;
      } else if (tx.type === 'sell' && runningQty > 0) {
        // Only the held quantity can realize a gain.
        const sold = Math.min(qty, runningQty);
        const avgCost = runningCost / runningQty;
        totalRealizedGain += (price - avgCost) * sold;
        runningCost -= avgCost * sold;
        runningQty -= sold;
      }
    }
  }

  return totalRealizedGain;
}
