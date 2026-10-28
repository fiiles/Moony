/**
 * The rows of the Cashflow screen's "Výdaje podle kategorií" table: the cashflow report's expense
 * groups joined with the budgeting report's summaries of the same period, accounts and report
 * currency (spec `docs/specs/2026-10-28-cashflow-with-budgets-implementation-design.md` §3).
 * Amounts, counts and shares are the cashflow report's; a limit is evaluated only from the
 * budget's own native values, so changing the report currency never changes it.
 */
import { isoMinorUnits } from '@shared/currencies';
import type { BudgetGoal, CashflowGroup, CategorySpendingSummary } from '@shared/schema';
import { absoluteMovementShares } from '@/utils/money-flows';

/** Over the limit, used exactly, from 85 %, below that, or not evaluable. */
export type LimitState = 'over' | 'exhausted' | 'near' | 'ok' | 'unavailable';

export interface RowLimit {
  state: LimitState;
  /** The budget's own currency (`budgetCurrency`); null only when unavailable without one. */
  currency: string | null;
  /** Spent against the limit, in `currency`. */
  spent: number | null;
  /** The limit for the whole period, in `currency` (kept when only the spend is unknown). */
  limit: number | null;
  /** `spent / limit` in per cent, as the backend works it out. */
  percentage: number | null;
  /** `limit − spent` in `currency` (negative when over); null when unavailable. */
  remaining: number | null;
}

export interface ExpenseRow {
  /** Category id, or `uncategorized`. */
  key: string;
  /** The stored name (empty for `uncategorized`); the screen names categories itself. */
  name: string;
  /** Net spending in the report currency: refunds lower it and can make it negative. */
  amount: number;
  /** Booked transactions behind the line. */
  count: number;
  /**
   * Every transaction of the line lacks a report conversion: `amount` is not a zero spend but
   * unknown (the cashflow report has no group for such a line, the budgeting report counts it).
   */
  amountUnavailable: boolean;
  /** Share of the absolute movement of all rows, 0–1. */
  share: number;
  /** The period has a limit for this category (evaluable or not). */
  hasLimit: boolean;
  /** The budget the report found for the period: a copy without the stored versions. */
  periodGoal: BudgetGoal | null;
  limit: RowLimit | null;
}

export type ExpenseFilter = 'all' | 'over' | 'none';
export type ExpenseSort = 'amount' | 'usage' | 'name';

const NEAR_PERCENT = 85;

function minorUnits(value: string, currency: string): number | null {
  const number = Number(value);
  if (value.trim() === '' || !Number.isFinite(number)) return null;
  return Math.round(number * 10 ** isoMinorUnits(currency));
}

/** How far the period's limit is used, from the budget's native spend and limit only. */
export function limitState(
  summary: Pick<
    CategorySpendingSummary,
    'budgetCurrency' | 'budgetSpent' | 'budgetLimitNative' | 'budgetPercentage'
  >
): RowLimit {
  const { budgetCurrency: currency, budgetSpent, budgetLimitNative, budgetPercentage } = summary;
  // The limit itself may still be known (the spend could not be converted into its currency).
  const knownLimit =
    currency && budgetLimitNative != null ? minorUnits(budgetLimitNative, currency) : null;
  const unavailable: RowLimit = {
    state: 'unavailable',
    currency: currency || null,
    spent: null,
    limit: currency && knownLimit != null ? knownLimit / 10 ** isoMinorUnits(currency) : null,
    percentage: null,
    remaining: null,
  };
  if (
    !currency ||
    budgetSpent == null ||
    budgetLimitNative == null ||
    budgetPercentage == null ||
    !Number.isFinite(budgetPercentage)
  ) {
    return unavailable;
  }
  const spent = minorUnits(budgetSpent, currency);
  const limit = minorUnits(budgetLimitNative, currency);
  if (spent == null || limit == null || limit <= 0) return unavailable;
  const scale = 10 ** isoMinorUnits(currency);
  const state: LimitState =
    spent > limit
      ? 'over'
      : spent === limit
        ? 'exhausted'
        : budgetPercentage >= NEAR_PERCENT
          ? 'near'
          : 'ok';
  return {
    state,
    currency,
    spent: spent / scale,
    limit: limit / scale,
    percentage: budgetPercentage,
    remaining: (limit - spent) / scale,
  };
}

/**
 * The cashflow expense groups (a group without transactions and amount is left out) plus every
 * category the budgeting report gives a limit for the period but the cashflow report has no group
 * for (a limit with no spending). Joined by category id; a line is never counted twice.
 */
export function expenseRows(
  groups: readonly CashflowGroup[],
  summaries: readonly CategorySpendingSummary[]
): ExpenseRow[] {
  const budgets = new Map(summaries.map((summary) => [summary.categoryId, summary]));
  const lines = groups
    .map((group) => ({
      key: group.key,
      name: group.name,
      amount: Number(group.amount) || 0,
      count: group.count,
      amountUnavailable: false,
    }))
    .filter((line) => line.count > 0 || line.amount !== 0);
  const listed = new Set(lines.map((line) => line.key));
  for (const summary of summaries) {
    if (summary.budgetGoal == null || listed.has(summary.categoryId)) continue;
    lines.push({
      key: summary.categoryId,
      name: summary.categoryName,
      amount: Number(summary.totalAmount) || 0,
      count: summary.transactionCount,
      amountUnavailable: summary.transactionCount > 0,
    });
  }
  const shares = absoluteMovementShares(lines.map((line) => line.amount));
  return lines.map((line, index) => {
    const budget = budgets.get(line.key);
    const periodGoal = budget?.budgetGoal ?? null;
    return {
      ...line,
      share: shares[index],
      hasLimit: periodGoal != null,
      periodGoal,
      limit: periodGoal && budget ? limitState(budget) : null,
    };
  });
}

/** How many rows each filter shows, over the whole overview. */
export function filterCounts(rows: readonly ExpenseRow[]): Record<ExpenseFilter, number> {
  return {
    all: rows.length,
    over: rows.filter((row) => row.limit?.state === 'over').length,
    none: rows.filter((row) => !row.hasLimit).length,
  };
}

export function filterRows(rows: readonly ExpenseRow[], filter: ExpenseFilter): ExpenseRow[] {
  if (filter === 'over') return rows.filter((row) => row.limit?.state === 'over');
  if (filter === 'none') return rows.filter((row) => !row.hasLimit);
  return [...rows];
}

/** Evaluated limits first, then limits that cannot be evaluated, then rows without one. */
function usageRank(row: ExpenseRow): number {
  if (!row.hasLimit) return 2;
  return row.limit?.state === 'unavailable' ? 1 : 0;
}

/**
 * A sorted copy. Ties fall back to the amount and then the key, so the order never depends on
 * the order the reports arrived in; a missing usage is never read as 0 %.
 */
export function sortRows(
  rows: readonly ExpenseRow[],
  sort: ExpenseSort,
  label: (row: ExpenseRow) => string,
  locale: string
): ExpenseRow[] {
  const byAmount = (a: ExpenseRow, b: ExpenseRow) =>
    b.amount - a.amount || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0);
  const compare = (a: ExpenseRow, b: ExpenseRow): number => {
    if (sort === 'name') {
      return label(a).localeCompare(label(b), locale) || byAmount(a, b);
    }
    if (sort === 'usage') {
      const rank = usageRank(a) - usageRank(b);
      if (rank !== 0) return rank;
      if (usageRank(a) === 0) {
        const usage = (b.limit?.percentage ?? 0) - (a.limit?.percentage ?? 0);
        if (usage !== 0) return usage;
      }
    }
    return byAmount(a, b);
  };
  return [...rows].sort(compare);
}
