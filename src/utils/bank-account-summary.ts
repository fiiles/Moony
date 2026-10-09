import { MissingTracker, type ConvertFn } from '@shared/fx';

export interface BalanceItem {
  balance?: string | null;
  currency?: string | null;
  /** Left out of the portfolio (net worth, dashboard totals) — see `exclude_from_balance`. */
  excludeFromBalance?: boolean;
  /** The account's money flow over the last 30 days, in its own currency. */
  last30?: { income: number; expense: number };
}

export interface AccountTotals {
  /** Balances of the accounts that count toward the portfolio, main currency. */
  included: number;
  /** Balances of the accounts the user excluded, main currency. */
  excluded: number;
  excludedCount: number;
  /** 30-day income and expense of the included accounts, main currency. */
  income: number;
  expense: number;
  /** Currencies of accounts left out of the totals for want of a rate. */
  missing: string[];
}

/**
 * Sum account balances in the main currency, split into the accounts that count toward
 * the portfolio and those the user excluded. The dashboard leaves excluded accounts out
 * of the net worth, so the Bank accounts summary must too, or the two disagree by
 * exactly their balance. An account whose currency has no rate stays out of every total
 * and is named in `missing`.
 */
export function accountTotals(items: readonly BalanceItem[], convert: ConvertFn): AccountTotals {
  const totals = { included: 0, excluded: 0, excludedCount: 0, income: 0, expense: 0 };
  const tracker = new MissingTracker();
  for (const item of items) {
    const currency = item.currency || 'CZK';
    if (item.excludeFromBalance) totals.excludedCount += 1;
    const amount = parseFloat(item.balance || '0');
    const rate = tracker.track(currency, convert(1, currency));
    if (rate === null) continue;
    const balance = (Number.isFinite(amount) ? amount : 0) * rate;
    if (item.excludeFromBalance) {
      totals.excluded += balance;
      continue;
    }
    totals.included += balance;
    totals.income += (item.last30?.income ?? 0) * rate;
    totals.expense += (item.last30?.expense ?? 0) * rate;
  }
  return { ...totals, missing: tracker.missing };
}
