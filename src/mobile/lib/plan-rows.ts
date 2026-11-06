/**
 * What the phone's spending-plan screens show (spec 2026-11-06-mobile-spending-plans): the
 * Cashflow card's summary, the list's sections, the picker's groups, the detail's payment filter
 * and the take-over inserts. The plan arithmetic (status, ratio, month buckets, sorting) stays in
 * `@/utils/spending-plans`, shared with the desktop; this only decides what a phone screen lists.
 */
import type {
  BankTransaction,
  InsertSpendingPlan,
  SpendingPlan,
  SpendingPlanSummary,
  SpendingPlansOverview,
} from '@shared/schema';
import {
  categoryKey,
  matchesPlanSearch,
  monthOfDay,
  overviewRows,
  planRow,
  takeOverRows,
  type PlanRow,
} from '@/utils/spending-plans';

/** The plan count above which the list (and from which the picker) shows a search field. */
export const SEARCH_FROM = 8;

/**
 * The Cashflow card: the year's totals in the report currency and how many active plans are over
 * or near their amount. `planned` and `spent` cover the whole year (closed plans too) while
 * `left` is active plans only (spec M17), so the screens label them as separate facts. A total
 * that could not be converted stays null. Null when the year has no plans (no card).
 */
export interface PlansCardSummary {
  year: number;
  currency: string;
  planned: number | null;
  spent: number | null;
  left: number | null;
  over: number;
  near: number;
}

const numberOrNull = (value: string | null): number | null => {
  if (value === null) return null;
  const parsed = parseFloat(value);
  return Number.isFinite(parsed) ? parsed : null;
};

export function plansCardSummary(overview: SpendingPlansOverview): PlansCardSummary | null {
  if (overview.plans.length === 0) return null;
  const active = overview.plans.map(planRow).filter((row) => !row.closed);
  return {
    year: overview.year,
    currency: overview.currency,
    planned: numberOrNull(overview.totals.planned),
    spent: numberOrNull(overview.totals.spent),
    left: numberOrNull(overview.totals.remainingActive),
    over: active.filter((row) => row.status === 'over').length,
    near: active.filter((row) => row.status === 'near').length,
  };
}

/** The list's two sections, each filtered by the name search and in the risk order. */
export function planListSections(
  rows: PlanRow[],
  query: string,
  locale: string
): { active: PlanRow[]; closed: PlanRow[] } {
  return {
    active: overviewRows(rows, 'active', query, locale),
    closed: overviewRows(rows, 'closed', query, locale),
  };
}

/**
 * The plan picker's groups: open plans only, the payment's booking year first, then the other
 * years newest first; names sorted in `locale`, filtered by the name search. Empty groups are
 * dropped (so a booking year without plans gets no heading).
 */
export function pickerGroups(
  index: SpendingPlanSummary[],
  bookingYear: number,
  query: string,
  locale: string
): { year: number; rows: PlanRow[] }[] {
  const byYear = new Map<number, PlanRow[]>();
  for (const summary of index) {
    if (summary.plan.closedAt !== null) continue;
    if (!matchesPlanSearch(summary.plan.name, query)) continue;
    const rows = byYear.get(summary.plan.year) ?? [];
    rows.push(planRow(summary));
    byYear.set(summary.plan.year, rows);
  }
  const years = [...byYear.keys()].sort((a, b) => {
    if (a === bookingYear) return -1;
    if (b === bookingYear) return 1;
    return b - a;
  });
  return years.map((year) => ({
    year,
    rows: (byYear.get(year) ?? []).sort((a, b) =>
      a.summary.plan.name.localeCompare(b.summary.plan.name, locale)
    ),
  }));
}

/**
 * The plan detail's payments narrowed by the month ("YYYY-MM" of the booking day) and the category
 * filter (a `categoryKey`, so `UNCATEGORIZED_KEY` keeps payments without a category). A null
 * filter does not narrow.
 */
export function filterPlanPayments(
  txs: BankTransaction[],
  month: string | null,
  category: string | null
): BankTransaction[] {
  return txs.filter(
    (tx) =>
      (month === null || monthOfDay(tx.bookingDate) === month) &&
      (category === null || categoryKey(tx.categoryId) === category)
  );
}

/**
 * The plans to create when taking `source` over into `toYear`: the open ones whose name is not
 * taken in the target year yet (the desktop's preselection, via `takeOverRows`).
 */
export function takeOverInserts(
  source: SpendingPlan[],
  targetNames: string[],
  toYear: number,
  locale: string
): InsertSpendingPlan[] {
  return takeOverRows(source, targetNames, locale)
    .filter((row) => row.selected)
    .map(({ plan }) => ({
      name: plan.name,
      year: toYear,
      amount: plan.amount,
      currency: plan.currency,
      icon: plan.icon,
      color: plan.color,
      note: plan.note,
    }));
}

/** The years the plan form offers: the year with its neighbours. */
export function yearChoices(year: number): number[] {
  return [year - 1, year, year + 1];
}
