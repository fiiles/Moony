/**
 * What the phone's plan detail draws (spec 2026-11-06-mobile-spending-plans M9): the month bars
 * and the category shares. The plan arithmetic (month axis, month spend, category rows) stays in
 * `@/utils/spending-plans`, shared with the desktop; this only shapes it for the phone's marks.
 */
import type { PlanCategoryRow } from '@/utils/spending-plans';

/** One month of "Útrata po měsících", in the plan currency. */
export interface PlanBar {
  /** "YYYY-MM". */
  month: string;
  /** Net spend of the month; negative when refunds outweigh payments. */
  value: number;
  /** Another month is selected: this one is drawn muted. */
  muted: boolean;
}

/**
 * The bars over `axis` (`planMonthAxis`) with their values (`planMonthSpend`, a missing value is
 * 0). With a selected month every other month is muted; without one none is.
 */
export function planBars(
  axis: readonly string[],
  values: readonly number[],
  selected: string | null
): PlanBar[] {
  return axis.map((month, index) => ({
    month,
    value: values[index] ?? 0,
    muted: selected !== null && month !== selected,
  }));
}

/** Axis labels with only every `every`-th one kept (from the first), the rest blank. */
export function sparseLabels(labels: readonly string[], every: number): string[] {
  return labels.map((label, index) => (index % every === 0 ? label : ''));
}

/** A category row with its share of the plan's spend, 0–1 (more than 1 beside a refund category). */
export interface PlanCategoryShare extends PlanCategoryRow {
  share: number;
}

/**
 * Each category's share of the rows' total for its bar, as the desktop's Kategorie card works it
 * out: a category of refunds (negative spend) gets 0, and so does every row when the total is not
 * positive.
 */
export function categoryShares(rows: readonly PlanCategoryRow[]): PlanCategoryShare[] {
  const total = rows.reduce((sum, row) => sum + row.spent, 0);
  return rows.map((row) => ({ ...row, share: total > 0 ? Math.max(0, row.spent / total) : 0 }));
}
