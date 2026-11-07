import type { BankAccount, BankTransaction } from '@shared/schema';

/**
 * Whether the account's `···` offers "Úročení" (the desktop's `InterestDialog`): the desktop's own
 * rule — the account has interest zones, or a simple rate above 0.
 */
export function showsInterest(
  account: Pick<BankAccount, 'hasZoneDesignation' | 'interestRate'>
): boolean {
  return account.hasZoneDesignation || (parseFloat(account.interestRate ?? '0') || 0) > 0;
}

/**
 * Whether "Automatická kategorizace" has anything to do: a payment of the account without a
 * category. Without one the menu item says so (the desktop's "all done" reason) instead of running.
 */
export function hasUncategorized(history: readonly Pick<BankTransaction, 'categoryId'>[]): boolean {
  return history.some((tx) => !tx.categoryId);
}
