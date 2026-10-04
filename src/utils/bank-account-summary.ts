export interface BalanceItem {
  balance?: string | null;
  currency?: string | null;
  /** Left out of the portfolio (net worth, dashboard totals) — see `exclude_from_balance`. */
  excludeFromBalance?: boolean;
}

/**
 * Sum account balances in CZK, split into the accounts that count toward the
 * portfolio and those the user excluded. The dashboard leaves excluded accounts
 * out of the net worth, so the Bank accounts summary must too, or the two
 * disagree by exactly their balance.
 */
export function splitBalancesCzk(
  items: readonly BalanceItem[],
  toCzk: (amount: number, currency: string) => number
): { included: number; excluded: number; excludedCount: number } {
  let included = 0;
  let excluded = 0;
  let excludedCount = 0;
  for (const item of items) {
    const amount = parseFloat(item.balance || '0');
    const czk = toCzk(Number.isFinite(amount) ? amount : 0, item.currency || 'CZK');
    if (item.excludeFromBalance) {
      excluded += czk;
      excludedCount += 1;
    } else {
      included += czk;
    }
  }
  return { included, excluded, excludedCount };
}
