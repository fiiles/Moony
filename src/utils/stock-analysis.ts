import type { StockInvestmentWithTags } from '@shared/schema';
import { amountOf } from '@/utils/positions';

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
  /** Current value of those whose value and cost are known, in the report currency. */
  value: number;
  /** Their gain or loss, report currency. */
  gainLoss: number;
  /** Gain or loss in percent of the cost basis (value − gain); 0 without a cost basis. */
  gainLossPercent: number;
  /** Their estimated yearly dividend, report currency. */
  dividend: number;
}

/** What the sums need of a position (see `toAnalysisPosition`); `null` = not convertible. */
interface TotalsPosition {
  tags: readonly { id: string }[];
  currentValue: number | null;
  gainLoss: number | null;
  yearlyDividendAmount: number | null;
}

/** A stocks-analysis row with its converted amounts as numbers (`null` when not convertible). */
export interface AnalysisPosition extends StockInvestmentWithTags {
  currentValue: number | null;
  gainLoss: number | null;
  gainLossPercent: number | null;
  yearlyDividendAmount: number | null;
}

export function toAnalysisPosition(item: StockInvestmentWithTags): AnalysisPosition {
  return {
    ...item,
    currentValue: amountOf(item.value),
    gainLoss: amountOf(item.unrealizedGain),
    gainLossPercent: item.unrealizedGainPercent,
    yearlyDividendAmount: amountOf(item.yearlyDividend),
  };
}

/**
 * Sums over `positions` with the rule of the backend's `PositionTotals`: a position adds its
 * value, gain and dividend only when its value and gain (so its cost) are known; it is counted
 * either way. Cost = value − gain, percent = gain / cost.
 */
export function positionTotals(positions: readonly TotalsPosition[]): TagTotals {
  let count = 0;
  let value = 0;
  let gainLoss = 0;
  let dividend = 0;
  for (const position of positions) {
    count += 1;
    if (position.currentValue === null || position.gainLoss === null) continue;
    value += position.currentValue;
    gainLoss += position.gainLoss;
    dividend += position.yearlyDividendAmount ?? 0;
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

/**
 * Sums over the positions carrying `tagId` (see `positionTotals`), so the tag table can follow
 * the chips with the very numbers it shows without them.
 */
export function tagTotals(positions: readonly TotalsPosition[], tagId: string): TagTotals {
  return positionTotals(positions.filter((p) => p.tags.some((tag) => tag.id === tagId)));
}
