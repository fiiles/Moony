import type { StockInvestmentWithTags, StockTag } from '@shared/schema';
import { amountOf } from '@/utils/positions';

/**
 * Selection and totals behind the stocks analysis page: which positions the tag chips select,
 * and the sums per tag over any such selection. Pure, so the page stays wiring.
 */

/** The value of a tag group that is not filtering. */
export const FILTER_ALL = 'all';

/**
 * Whether a position is still held. The analysis report lists every position of the account,
 * sold-out ones (quantity 0) included, while the performance engine covers only held positions.
 */
export function isHeld(position: { quantity: string }): boolean {
  return Number(position.quantity) > 0;
}

/** The held positions in their order: what the whole analysis page shows (see `isHeld`). */
export function heldPositions<T extends { quantity: string }>(positions: readonly T[]): T[] {
  return positions.filter(isHeld);
}

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

/**
 * The tags of one position after choosing in a tag group: the group is a single choice, so the
 * tag the position has there is replaced by `tagId`, or just removed for `null` ("Bez štítku").
 * Tags of other groups and tags without a group stay. A `tagId` that is not a tag of the group
 * (deleted or moved meanwhile) changes nothing.
 */
export function pickGroupTag(
  tagIds: readonly string[],
  tags: readonly StockTag[],
  groupId: string,
  tagId: string | null
): string[] {
  if (tagId !== null && !tags.some((tag) => tag.id === tagId && tag.groupId === groupId)) {
    return [...tagIds];
  }
  const inGroup = new Set(tags.filter((tag) => tag.groupId === groupId).map((tag) => tag.id));
  const next = tagIds.filter((id) => !inGroup.has(id));
  if (tagId !== null) next.push(tagId);
  return next;
}

/**
 * The tags of one position after clicking a tag without a group: tags without a group combine
 * freely, so the click adds the tag or, if the position has it, removes it. A tag that is not
 * one without a group (it belongs to a group, or does not exist) changes nothing.
 */
export function toggleFreeTag(
  tagIds: readonly string[],
  tags: readonly StockTag[],
  tagId: string
): string[] {
  if (!tags.some((tag) => tag.id === tagId && tag.groupId === null)) return [...tagIds];
  return tagIds.includes(tagId) ? tagIds.filter((id) => id !== tagId) : [...tagIds, tagId];
}

/**
 * The positions whose draft tags differ from the saved ones (the order of the tags does not
 * matter), in the order of `positions`. A position without a draft is unchanged, and so is a
 * draft of a position that is not in the list.
 */
export function changedPositions(
  positions: readonly { id: string; tags: readonly { id: string }[] }[],
  drafts: ReadonlyMap<string, readonly string[]>
): string[] {
  const key = (ids: readonly string[]) => [...ids].sort().join('\n');
  return positions
    .filter((position) => {
      const draft = drafts.get(position.id);
      return draft !== undefined && key(draft) !== key(position.tags.map((tag) => tag.id));
    })
    .map((position) => position.id);
}

/**
 * The position the assign dialog opens on: the first of `preferredIds` that is in the list,
 * else the first position without a tag, else the first position; `null` for no positions.
 */
export function initialActiveId(
  positions: readonly { id: string; tags: readonly unknown[] }[],
  preferredIds: readonly string[]
): string | null {
  const preferred = preferredIds.find((id) => positions.some((position) => position.id === id));
  if (preferred !== undefined) return preferred;
  return (positions.find((position) => position.tags.length === 0) ?? positions[0])?.id ?? null;
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
