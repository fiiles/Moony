import type { CategoryCount } from '@shared/schema';

interface BreakdownLabels {
  /** Label of the bucket without a category ("bez kategorie"). */
  uncategorized: string;
  /** Label for the folded tail ("+2 dalších"). */
  more: (count: number) => string;
}

/**
 * "12× Stravování · 2× bez kategorie · +1 dalších": the `max` largest buckets of a
 * recategorization preview, the rest folded into one count.
 */
export function formatBreakdown(
  byCategory: readonly CategoryCount[],
  nameOf: (categoryId: string) => string,
  labels: BreakdownLabels,
  max = 3
): string {
  const sorted = [...byCategory].sort((a, b) => b.count - a.count);
  const parts = sorted
    .slice(0, max)
    .map((b) => `${b.count}× ${b.categoryId ? nameOf(b.categoryId) : labels.uncategorized}`);
  const rest = sorted.length - max;
  if (rest > 0) parts.push(labels.more(rest));
  return parts.join(' · ');
}

/** Payments a pick would actually change: every match minus those already in `categoryId`. */
export function changedByPick(
  preview: { matches: number; byCategory: readonly CategoryCount[] },
  categoryId: string
): number {
  const same = preview.byCategory.find((b) => b.categoryId === categoryId)?.count ?? 0;
  return Math.max(0, preview.matches - same);
}
