/**
 * Selection and totals behind the stocks analysis page: which positions the tag chips select,
 * and the sums per tag over any such selection. Pure, so the page stays wiring.
 */

/** The value of a tag group that is not filtering. */
export const FILTER_ALL = 'all';

/** One row of chips: a group of tags of which one can be chosen. */
export interface ChipGroup {
  key: string;
  tags: readonly { id: string }[];
}

/**
 * The tags the chips select, one per group that filters. A choice that is no longer one of its
 * group's tags (deleted meanwhile) is ignored, so a stale chip cannot empty the whole page.
 */
export function chosenTagIds(
  groups: readonly ChipGroup[],
  filters: Readonly<Record<string, string>>
): string[] {
  const ids: string[] = [];
  for (const group of groups) {
    const chosen = filters[group.key];
    if (chosen && chosen !== FILTER_ALL && group.tags.some((tag) => tag.id === chosen)) {
      ids.push(chosen);
    }
  }
  return ids;
}

/**
 * The positions that carry every chosen tag (one tag per group, so AND across groups). Choosing
 * nothing selects every position.
 */
export function selectPositions<T extends { tags: readonly { id: string }[] }>(
  positions: readonly T[],
  tagIds: readonly string[]
): T[] {
  if (tagIds.length === 0) return [...positions];
  return positions.filter((position) =>
    tagIds.every((id) => position.tags.some((tag) => tag.id === id))
  );
}

export interface TagTotals {
  /** Positions carrying the tag. */
  count: number;
  /** Their current value, CZK. */
  value: number;
  /** Their gain or loss, CZK. */
  gainLoss: number;
  /** Gain or loss in percent of the cost basis (value − gain); 0 without a cost basis. */
  gainLossPercent: number;
  /** Their estimated yearly dividend, CZK. */
  dividend: number;
}

/** What the sums need of a position; a `StockInvestmentWithTags` has all of it. */
interface TotalsPosition {
  tags: readonly { id: string }[];
  currentValue: number;
  gainLoss: number;
  dividendYield: number;
}

/**
 * Sums over the positions carrying `tagId`. The formulas of the backend's `get_tag_metrics`
 * (cost = value − gain, percent = gain / cost), applied to any subset of positions, so the tag
 * table can follow the chips with the very numbers it shows without them.
 */
export function tagTotals(positions: readonly TotalsPosition[], tagId: string): TagTotals {
  let count = 0;
  let value = 0;
  let gainLoss = 0;
  let dividend = 0;
  for (const position of positions) {
    if (!position.tags.some((tag) => tag.id === tagId)) continue;
    count += 1;
    value += position.currentValue;
    gainLoss += position.gainLoss;
    dividend += position.dividendYield;
  }
  const cost = value - gainLoss;
  return {
    count,
    value,
    gainLoss,
    gainLossPercent: cost > 0 ? (gainLoss / cost) * 100 : 0,
    dividend,
  };
}
