/**
 * Historical-rate cost basis derivation from transactions.
 *
 * Cost basis is never stored — it is derived from the transaction record,
 * converting every transaction at the exchange rate of its own day
 * (ADR 0001: "converting old values at today's rate is wrong").
 * Weighted-average-cost semantics: buys accumulate cost, sells release cost
 * proportionally at the running average — mirroring the Rust engine in
 * src-tauri/src/services/cost_basis.rs.
 */

import type { CurrencyCode } from '../currencies';

export interface CostBasisTransaction {
  type: string; // 'buy' | 'sell'
  quantity: string;
  pricePerUnit: string;
  currency: string;
  transactionDate: number;
  /** Insertion time; breaks ties between same-day transactions when present. */
  createdAt?: number;
}

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
 * day (unix seconds). Implementations resolve the closest earlier day and
 * fall back to today's rates when no history covers the day.
 */
export type DatedConvertFn = (
  amount: number,
  from: CurrencyCode,
  to: CurrencyCode,
  dateTs: number
) => number;

export interface PositionCostBasis {
  /** Open quantity after replaying buys and sells. */
  quantity: number;
  /** Cost basis in the target currency, each transaction at its day's rate. */
  costBasis: number;
  /** costBasis / quantity (0 for a closed position). */
  averageCost: number;
}

/**
 * Replay one position's transactions into a historical-rate cost basis in
 * `targetCurrency`. Transactions are processed chronologically (see
 * `compareChronologically`), matching the Rust engine.
 */
export function calculatePositionCostBasis(
  transactions: CostBasisTransaction[],
  convertAt: DatedConvertFn,
  targetCurrency: CurrencyCode
): PositionCostBasis {
  const ordered = [...transactions].sort(compareChronologically);

  let quantity = 0;
  let costBasis = 0;

  for (const tx of ordered) {
    const qty = parseFloat(tx.quantity) || 0;
    const price = parseFloat(tx.pricePerUnit) || 0;

    if (tx.type === 'buy') {
      const amount =
        tx.currency === targetCurrency
          ? qty * price
          : convertAt(qty * price, tx.currency as CurrencyCode, targetCurrency, tx.transactionDate);
      costBasis += amount;
      quantity += qty;
    } else if (tx.type === 'sell') {
      // Release cost proportionally at the running average. A sell larger than
      // the open quantity (legacy data, or a date edit that moved a sell before
      // its buy) only closes what is held.
      const sold = Math.min(qty, Math.max(quantity, 0));
      if (quantity > 0 && sold > 0) {
        costBasis -= costBasis * (sold / quantity);
      }
      quantity -= sold;
    }
  }

  if (quantity < 0) quantity = 0;
  if (costBasis < 0) costBasis = 0;
  if (quantity === 0) costBasis = 0; // a fully closed position holds no cost

  return {
    quantity,
    costBasis,
    averageCost: quantity > 0 ? costBasis / quantity : 0,
  };
}
