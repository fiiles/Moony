import type {
  CashflowGroup,
  CashflowMonth,
  CategorySpendingSummary,
  FlowKind,
} from '@shared/schema';
import type { NamedCategory } from '@/utils/category-name';
import { absoluteMovementShares } from '@/utils/money-flows';
import { summarizeBudgetStatus } from '@/utils/report-currencies';

/** How much of its budget limit a category has used. */
export interface RowLimit {
  /** Spent against the limit, in the currency of the limit (not the report currency). */
  spent: number;
  /** The limit for the period, in the currency of the limit. */
  limit: number;
  /** The currency of the limit (`budgetCurrency`). */
  currency: string;
  /** `spent / limit` in per cent, as the backend works it out. */
  percentage: number;
  /** Over 100 % is `over`, from 85 % `near`, else `ok` (the thresholds of `summarizeBudgetStatus`). */
  state: 'ok' | 'near' | 'over';
}

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
  limit: RowLimit | null;
}

const NO_TARGET = '';

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

function rowLimit(summary: CategorySpendingSummary | undefined): RowLimit | null {
  if (
    !summary ||
    summary.budgetGoal == null ||
    !summary.budgetCurrency ||
    summary.budgetSpent == null ||
    summary.budgetLimitNative == null ||
    summary.budgetPercentage == null ||
    !Number.isFinite(summary.budgetPercentage)
  ) {
    return null;
  }
  const status = summarizeBudgetStatus([summary], NO_TARGET);
  return {
    spent: Number(summary.budgetSpent),
    limit: Number(summary.budgetLimitNative),
    currency: summary.budgetCurrency,
    percentage: summary.budgetPercentage,
    state: status.over > 0 ? 'over' : status.near > 0 ? 'near' : 'ok',
  };
}

/**
 * The rows of one breakdown (expenses, income or transfers), in the order of the groups (the
 * backend sorts them largest first). A group with no transactions and no amount is left out; the
 * shares are over what is left. The budget summary of the same category (`budgets` is keyed by
 * category id) supplies the limit, when it has an evaluable one.
 */
export function categoryRows(
  groups: readonly CashflowGroup[],
  budgets?: ReadonlyMap<string, CategorySpendingSummary>
): CategoryRowModel[] {
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
    limit: rowLimit(budgets?.get(group.key)),
  }));
}

export interface BudgetCounts {
  ok: number;
  near: number;
  over: number;
}

/**
 * How many budgets are within their limit, near it and over it, for the note on the Cashflow tab.
 * Only categories with a budget goal count, and only those whose limit can be evaluated; `null`
 * when there are none, so the note stays away.
 */
export function budgetCounts(summaries: readonly CategorySpendingSummary[]): BudgetCounts | null {
  const status = summarizeBudgetStatus(
    summaries.filter((summary) => summary.budgetGoal != null),
    NO_TARGET
  );
  if (status.onTrack + status.near + status.over === 0) return null;
  return { ok: status.onTrack, near: status.near, over: status.over };
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
