/**
 * The rows of the budgets page (spec §8.7, the phone counterpart of the desktop's
 * `BudgetManageDialog` and `Budgeting` page): which budgets are listed and in which order, which
 * of them may lose a single interval, which categories are still free for a limit and which of
 * those the "Výdaje bez limitu" section lists. The version arithmetic stays in
 * `@/utils/budget-goal-versions`; this decides what the page shows.
 */
import type { BudgetGoal, BudgetGoalVersion } from '@shared/schema';
import {
  budgetOverview,
  type BudgetLimitPeriod,
  type BudgetOverviewStatus,
} from '@/utils/budget-goal-versions';
import type { LimitMode } from '@/mobile/sheets/BudgetLimitSheet';
import type { LimitAverage } from './budget-limit';

/** One budget of the list: the limit that stands for it and what changes after it. */
export interface BudgetRow {
  /** `categoryId:timeframe`, unique among the rows. */
  key: string;
  goal: BudgetGoal;
  categoryId: string;
  /** The category's name as the app names it. */
  name: string;
  status: BudgetOverviewStatus;
  /** The interval shown: in effect now, the first scheduled one, or the last of an ended budget. */
  main: BudgetLimitPeriod;
  /** Changes after `main` starts (a null amount ends the limit); empty for an ended budget. */
  upcoming: BudgetGoalVersion[];
  /** Another limit than the one shown exists, so deleting an interval is not deleting the budget. */
  canDeleteInterval: boolean;
}

/**
 * The budgets as rows: one per goal that has a limit, active and scheduled ones first and ended
 * ones last, each group by name (`localeCompare` in `locale`). An interval can be deleted when the
 * budget has an earlier limit or a later one (a scheduled removal is no limit): with a single
 * interval, deleting it would be deleting the budget (the desktop's rule).
 */
export function budgetRows(
  history: readonly BudgetGoal[],
  nowMonth: number,
  nameOf: (categoryId: string) => string,
  locale: string
): BudgetRow[] {
  const rows = budgetOverview(history, nowMonth).map((row): BudgetRow => ({
    key: `${row.goal.categoryId}:${row.goal.timeframe}`,
    goal: row.goal,
    categoryId: row.goal.categoryId,
    name: nameOf(row.goal.categoryId),
    status: row.status,
    main: row.main,
    upcoming: row.upcoming,
    canDeleteInterval:
      row.history.length + row.upcoming.filter((version) => version.amount != null).length > 0,
  }));
  const byName = (a: BudgetRow, b: BudgetRow) => a.name.localeCompare(b.name, locale);
  return [
    ...rows.filter((row) => row.status !== 'ended').sort(byName),
    ...rows.filter((row) => row.status === 'ended').sort(byName),
  ];
}

/**
 * The categories that have a monthly limit in effect or scheduled — the ones a new limit cannot
 * be added to. A stored goal alone is not enough: it outlives a limit that has run out, and that
 * category may get a new budget (the desktop page's `availableCategories` rule).
 */
export function budgetedCategoryIds(history: readonly BudgetGoal[], nowMonth: number): Set<string> {
  return new Set(
    budgetOverview(
      history.filter((goal) => goal.timeframe === 'monthly'),
      nowMonth
    )
      .filter((row) => row.status !== 'ended')
      .map((row) => row.goal.categoryId)
  );
}

/** An expense category that spends but has no limit: its average per month and its payments. */
export interface UnbudgetedRow {
  categoryId: string;
  /** Average monthly spending over the months the report observed. */
  average: number;
  /** Payments behind it in the report's window; 0 when the report has none for the category. */
  count: number;
}

/**
 * "Výdaje bez limitu": the expense categories with an average above zero that are not budgeted,
 * the biggest spenders first. `counts` are the report's `transactionCount` by category id;
 * `expenseCategoryIds` keeps income and transfer categories out (no limit can be set on them).
 */
export function unbudgetedRows(
  averages: ReadonlyMap<string, LimitAverage>,
  counts: ReadonlyMap<string, number>,
  budgeted: ReadonlySet<string>,
  expenseCategoryIds: ReadonlySet<string>
): UnbudgetedRow[] {
  const rows: UnbudgetedRow[] = [];
  for (const [categoryId, { average }] of averages) {
    if (average > 0 && !budgeted.has(categoryId) && expenseCategoryIds.has(categoryId)) {
      rows.push({ categoryId, average, count: counts.get(categoryId) ?? 0 });
    }
  }
  return rows.sort((a, b) => b.average - a.average);
}

/**
 * What the limit item of an expense row's menu on the Cashflow report does (ruling R15, the
 * desktop Budgets table's "Upravit limit" / "Nastavit limit"). `periodGoal` is the budget the
 * report found for the category in the shown period (`budgetGoal` of its summary — a historical
 * copy without the versions); the sheet edits the stored goal of the same timeframe from
 * `goals`, starting at the month the report shows. Without a budget in the period the item adds
 * one from that month. `mode` is `null` while the item cannot act: the period has a budget but
 * the stored goals have not loaded.
 */
export function reportRowLimit(
  categoryId: string,
  periodGoal: BudgetGoal | null | undefined,
  goals: readonly BudgetGoal[],
  periodStart: number
): { hasLimit: boolean; mode: LimitMode | null } {
  if (!periodGoal) {
    return { hasLimit: false, mode: { kind: 'add', categoryId, effectiveMonth: periodStart } };
  }
  const stored = goals.find(
    (goal) => goal.categoryId === categoryId && goal.timeframe === periodGoal.timeframe
  );
  return {
    hasLimit: true,
    mode: stored ? { kind: 'edit', goal: stored, effectiveMonth: periodStart } : null,
  };
}
