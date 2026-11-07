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
import { storedLimitGoal, type ExpenseRow } from '@/utils/cashflow-budget-rows';
import type { LimitAverage, LimitMode } from './budget-limit';

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
  /** Earlier intervals with a limit, newest first (the desktop dialog's "Dřívější limit" rows). */
  history: BudgetLimitPeriod[];
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
    history: row.history,
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

/** The limit action of an expense row on the Cashflow report. */
export interface RowLimitAction {
  /** The period has a limit for the row: the action shows the limit and edits it. */
  hasLimit: boolean;
  /** What the limit sheet opens on; `null` while the stored limits are not known (shown disabled). */
  mode: LimitMode | null;
}

/**
 * The limit action of an expense row (the desktop Cashflow's `openLimit`): the category's stored
 * budget — even one whose limit ended or starts later — is edited from the month the report shows
 * (the sheet then lists what a save replaces); a category that never had one gets a new limit
 * from that month. `periodGoal` is the report's copy without versions, used only when the stored
 * goal is missing. A row without a limit whose category cannot get one (uncategorized, an income
 * or transfer category: `canHaveLimit` false) has no action. While the stored limits load or
 * failed (`limitsReady` false) the action is there but cannot act: add and edit cannot be told
 * apart yet.
 */
export function rowLimitAction(
  row: Pick<ExpenseRow, 'key' | 'periodGoal' | 'hasLimit'>,
  canHaveLimit: boolean,
  goals: readonly BudgetGoal[],
  periodStart: number,
  limitsReady: boolean
): RowLimitAction | null {
  if (!row.hasLimit && !canHaveLimit) return null;
  if (!limitsReady) return { hasLimit: row.hasLimit, mode: null };
  const goal = storedLimitGoal(row, goals) ?? row.periodGoal;
  return {
    hasLimit: row.hasLimit,
    mode: goal
      ? { kind: 'edit', goal, effectiveMonth: periodStart }
      : { kind: 'add', categoryId: row.key, effectiveMonth: periodStart },
  };
}
