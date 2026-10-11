import type { StockInvestmentWithTags, StockTag } from '@shared/schema';
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
  /**
   * Known current value, in the report currency; `null` when priced positions hold value but
   * none of it converted (the backend's rule: unknown, not 0).
   */
  value: number | null;
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
  /** Native price; `null` when unpriced (no value to convert). */
  price: string | null;
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
 * Sums over `positions` with the rule of the backend's `PositionTotals`: every known amount
 * counts. The value sums the known values (and the dividends of those positions), `null` when
 * priced positions hold value but none converted; the gain sums the known gains, and its
 * percent is of the cost of those same positions (value − gain). Every position is counted.
 */
export function positionTotals(positions: readonly TotalsPosition[]): TagTotals {
  let count = 0;
  let value = 0;
  let valueKnown = false;
  let valueFailed = false;
  let gainLoss = 0;
  let gainedValue = 0;
  let dividend = 0;
  for (const position of positions) {
    count += 1;
    if (position.currentValue === null) {
      valueFailed ||= position.price !== null;
      continue;
    }
    valueKnown = true;
    value += position.currentValue;
    dividend += position.yearlyDividendAmount ?? 0;
    if (position.gainLoss === null) continue;
    gainLoss += position.gainLoss;
    gainedValue += position.currentValue;
  }
  const cost = gainedValue - gainLoss;
  return {
    count,
    value: valueKnown || !valueFailed ? value : null,
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

/** A group row left on "Beze změny": the position keeps whatever tag it has in that group. */
export const PICK_KEEP = 'keep';
/** A group row set to "Bez štítku": the position loses its tag in that group. */
export const PICK_NONE = 'none';

/**
 * The tags a position gets from the assign dialog. A tag group is one choice: per group
 * `picks[groupId]` is `'keep'` (the position keeps its tag there), `'none'` (removed) or the id
 * of a tag of that group (replaces the current one). A group the picks do not mention, or a pick
 * that is not a tag of its group any more, keeps the current tag. Tags without a group are
 * `free`: for a single position they are exactly the checked ones, for several positions the
 * checked ones are added to the free tags each position already has.
 */
export function nextTagIds(
  current: readonly StockTag[],
  picks: Readonly<Record<string, string>>,
  free: readonly string[],
  single: boolean,
  tags: readonly StockTag[]
): string[] {
  const result: string[] = [];
  const add = (id: string) => {
    if (!result.includes(id)) result.push(id);
  };
  const pickOf = (groupId: string): string | null => {
    const pick = picks[groupId];
    if (pick === undefined || pick === PICK_KEEP) return null;
    if (pick === PICK_NONE) return PICK_NONE;
    return tags.some((tag) => tag.id === pick && tag.groupId === groupId) ? pick : null;
  };

  for (const tag of current) {
    if (tag.groupId === null) {
      // Free tags are replaced by the checked ones for a single position, kept for several
      if (!single) add(tag.id);
    } else if (pickOf(tag.groupId) === null) {
      add(tag.id);
    }
  }
  for (const groupId of Object.keys(picks)) {
    const pick = pickOf(groupId);
    if (pick !== null && pick !== PICK_NONE) add(pick);
  }
  for (const id of free) {
    if (tags.some((tag) => tag.id === id && tag.groupId === null)) add(id);
  }
  return result;
}

/**
 * The positions that would carry two tags of `groupId` if tag `tagId` moved into it: those that
 * have the tag and already carry another tag of the group. Names, in the order of `positions`.
 */
export function moveConflicts(
  positions: readonly { companyName: string; tags: readonly StockTag[] }[],
  tagId: string,
  groupId: string
): string[] {
  return positions
    .filter(
      (position) =>
        position.tags.some((tag) => tag.id === tagId) &&
        position.tags.some((tag) => tag.id !== tagId && tag.groupId === groupId)
    )
    .map((position) => position.companyName);
}
