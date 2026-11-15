import type {
  BankAccount,
  CashflowActuals,
  CashflowGroup,
  CashflowMonth,
  FlowKind,
} from '@shared/schema';
import type { NamedCategory } from '@/utils/category-name';
import { absoluteMovementShares } from '@/utils/money-flows';

export interface CategoryRowModel {
  /** Category id, or `uncategorized`. */
  key: string;
  /** The name as stored (empty for `uncategorized`); the screen translates it. */
  name: string;
  flowKind: FlowKind;
  /** In the report currency; sign as the group has it. */
  amount: number;
  /** Booked rows behind the group. */
  count: number;
  /** Share of the movement of the listed groups, 0–1. */
  share: number;
}

/** The `key` of the group of payments that have no category. */
export const UNCATEGORIZED = 'uncategorized';

/** The words a category line is titled with (`useCategoryName()` and the two translated labels). */
export interface CategoryTitleLabels {
  categoryName: (category: NamedCategory) => string;
  uncategorized: string;
  uncategorizedTransfers: string;
}

/**
 * The title of a report line, for its row and for the drilldown behind it: `uncategorized` is
 * "Bez kategorie" (the transfers' own wording for transfers), any other key a category, named as
 * the app names categories (`category`, when it is known; else the stored `name` of the report's
 * group, a system category recognised by its `cat_` id).
 */
export function categoryTitle(
  key: string,
  flowKind: FlowKind,
  category: NamedCategory | null | undefined,
  name: string,
  labels: CategoryTitleLabels
): string {
  if (key === UNCATEGORIZED) {
    return flowKind === 'transfer' ? labels.uncategorizedTransfers : labels.uncategorized;
  }
  return labels.categoryName(category ?? { id: key, name, isSystem: key.startsWith('cat_') });
}

/**
 * The rows of the income or the transfer breakdown, in the order of the groups (the backend sorts
 * them largest first). A group with no transactions and no amount is left out; the shares are over
 * what is left. The expense rows are the desktop's (`expenseRows` of `@/utils/cashflow-budget-rows`).
 */
export function categoryRows(groups: readonly CashflowGroup[]): CategoryRowModel[] {
  const kept = groups
    .map((group) => ({ group, amount: Number(group.amount) || 0 }))
    .filter(({ group, amount }) => group.count > 0 || amount !== 0);
  const shares = absoluteMovementShares(kept.map(({ amount }) => amount));
  return kept.map(({ group, amount }, index) => ({
    key: group.key,
    name: group.name,
    flowKind: group.flowKind,
    amount,
    count: group.count,
    share: shares[index],
  }));
}

/** One month of the chart: both bars, dimmed when the month is outside the selected period. */
export interface BarDatum {
  /** Unix seconds of the first day of the month, UTC midnight. */
  month: number;
  income: number;
  expenses: number;
  muted: boolean;
}

/**
 * The bars of the twelve-month chart. The window always ends with the period's last month, so
 * for a month, quarter or year the months outside it stay on the chart but muted.
 */
export function barsFor(
  months: readonly CashflowMonth[],
  periodStart: number,
  periodEnd: number
): BarDatum[] {
  return months.map((entry) => ({
    month: entry.month,
    income: Number(entry.income) || 0,
    expenses: Number(entry.expenses) || 0,
    muted: entry.month < periodStart || entry.month > periodEnd,
  }));
}

/** The part of the income that was kept (`net / totalIncome`); `null` without income. */
export function savingsRate(totalIncome: number, net: number): number | null {
  return totalIncome > 0 ? net / totalIncome : null;
}

/** What the body of the Cashflow tab shows under the period bar. */
export type BodyKind = 'skeleton' | 'error' | 'noAccounts' | 'emptyPeriod' | 'report';

/**
 * Which body the Cashflow tab shows. The invitation to add an account is for a profile with no
 * account at all (`accountTotal`, from the accounts list): the report's own `accountCount` is the
 * accounts among the period's transactions, so it is 0 for every empty period and says nothing
 * about the profile. A period is empty only with no transaction and no limit (the desktop's rule:
 * limits alone keep the report, `hasLimits`); an empty one keeps the bars and says so
 * (`emptyPeriod`). A failed load is the error only while there is nothing to show; the skeleton
 * waits for both the report and the accounts.
 */
export function bodyKind({
  accountsLoaded,
  accountTotal,
  actuals,
  isError,
  hasLimits,
}: {
  accountsLoaded: boolean;
  accountTotal: number;
  actuals: CashflowActuals | undefined;
  isError: boolean;
  hasLimits: boolean;
}): BodyKind {
  if (!actuals && isError) return 'error';
  if (!actuals || !accountsLoaded) return 'skeleton';
  if (accountTotal === 0) return 'noAccounts';
  return actuals.transactionCount === 0 && !hasLimits ? 'emptyPeriod' : 'report';
}

/**
 * The account scope of the report as its filter states it: `null` for all accounts (no choice, or
 * every account chosen), else how many of the chosen accounts exist — never the number of accounts
 * that happen to have payments in the period. While the accounts are not loaded the choice counts
 * as it is.
 */
export function accountScope(
  accountIds: readonly string[] | undefined,
  accounts: readonly Pick<BankAccount, 'id'>[]
): number | null {
  if (!accountIds) return null;
  if (accounts.length === 0) return accountIds.length;
  const known = new Set(accounts.map((account) => account.id));
  const chosen = new Set(accountIds.filter((id) => known.has(id))).size;
  return chosen === known.size ? null : chosen;
}

/** How a bar's month stands under the twelve-month chart. */
export type BarLabel = 'strong' | 'shown' | 'hidden';

/**
 * Which months the bar chart names (audit 2026-11-15 D20): a month report names its own month,
 * strong, and every third month counted from it, so the chosen month is always labelled; a
 * quarter or a year keeps every third month from the first.
 */
export function barAxisLabels(bars: readonly Pick<BarDatum, 'muted'>[]): BarLabel[] {
  const chosen = bars.flatMap((bar, index) => (bar.muted ? [] : [index]));
  const anchor = chosen.length === 1 ? chosen[0] : 0;
  return bars.map((_, index) =>
    chosen.length === 1 && index === anchor
      ? 'strong'
      : (index - anchor) % 3 === 0
        ? 'shown'
        : 'hidden'
  );
}
