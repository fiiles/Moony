import type { TFunction } from 'i18next';

/**
 * English names the baseline migration seeds for the system categories.
 * A system category whose stored name still equals its seeded name is shown
 * translated; once the user renames it, their word wins.
 */
export const SYSTEM_CATEGORY_DEFAULT_NAMES: Readonly<Record<string, string>> = {
  cat_dining: 'Dining & Restaurants',
  cat_entertainment: 'Entertainment',
  cat_groceries: 'Groceries',
  cat_health: 'Health & Medical',
  cat_housing: 'Housing',
  cat_income: 'Income',
  cat_insurance: 'Insurance',
  cat_internal_transfers: 'Internal Transfers',
  cat_investments: 'Investments',
  cat_loan_payments: 'Loan Payments',
  cat_other: 'Other',
  cat_savings: 'Savings',
  cat_shopping: 'Shopping',
  cat_taxes: 'Taxes',
  cat_transport: 'Transportation',
  cat_travel: 'Travel',
  cat_utilities: 'Utilities',
};

export interface NamedCategory {
  id: string;
  name: string;
  isSystem?: boolean;
}

/** True while a system category still carries its seeded (translatable) name. */
export function hasSeededName(category: NamedCategory): boolean {
  if (!category.isSystem) return false;
  const seeded = SYSTEM_CATEGORY_DEFAULT_NAMES[category.id];
  return seeded === undefined || seeded === category.name;
}

/**
 * The name to show for a category. `t` must be bound to the `bank_accounts`
 * namespace, which holds `categoryNames.<id>`. Custom categories and renamed
 * system categories show their stored name; seeded system categories are
 * translated with the stored name as the fallback.
 */
export function categoryDisplayName(category: NamedCategory, t: TFunction): string {
  if (!hasSeededName(category)) return category.name;
  return t(`categoryNames.${category.id}`, { defaultValue: category.name });
}
