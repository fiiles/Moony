import type { StockTag } from '@shared/schema';
import { matchesSearch } from '@/utils/holdings-search';
import { concentration } from '@/utils/stock-performance';
import { positionTotals, type AnalysisPosition } from '@/utils/stock-analysis';
import { sumKnown } from './performance-model';

/** One row of chips: the tags of a tag group, or the tags without a group. */
export interface TagGroupRows {
  key: string;
  label: string;
  tags: readonly StockTag[];
  /** The tags without a group: they can overlap. */
  free?: boolean;
}

/** A row of the tags table. */
export interface TagRow {
  tag: StockTag;
  /** Name of its group; `null` for a tag without one. */
  groupName: string | null;
  /** Selected positions carrying the tag. */
  count: number;
  /** Their value as of the end of the period; `null` when none is known. */
  value: number | null;
  /** Value as a share of the whole portfolio at the end, ratio. */
  share: number | null;
  /** Return of the tag's positions over the period, percent; `null` without a series. */
  periodReturn: number | null;
  /** Unrealized gain as of today, in the report currency. */
  gain: number;
  /** Unrealized gain as of today, percent of cost. */
  gainPercent: number;
  /** Estimated dividend a year, as of today. */
  dividend: number;
}

export interface TagRowsInput {
  groups: readonly TagGroupRows[];
  /** The positions the chips select. */
  selected: readonly AnalysisPosition[];
  /** Value of a position as of the end of the period. */
  endValueOf: (positionId: string) => number | null;
  /** Value of the whole portfolio as of the end of the period. */
  endTotal: number | null;
  /** Return of a tag's series over the period, percent. */
  periodReturnOf: (tagId: string) => number | null;
  /** With a filter only tags that have a selected position are listed. */
  filterActive: boolean;
}

/**
 * Rows of the tags table: every tag of the tag groups in group order, then the tags without a
 * group. Values are as of the end of the period and their share is of the whole portfolio;
 * unrealized gain and dividends stay as of today.
 */
export function tagRows(input: TagRowsInput): TagRow[] {
  const rows: TagRow[] = [];
  for (const group of input.groups) {
    for (const tag of group.tags) {
      const members = input.selected.filter((p) => p.tags.some((x) => x.id === tag.id));
      if (input.filterActive && members.length === 0) continue;
      const totals = positionTotals(members);
      const value = sumKnown(members.map((p) => input.endValueOf(p.id)));
      rows.push({
        tag,
        groupName: group.free ? null : group.label,
        count: members.length,
        value,
        share:
          value !== null && input.endTotal !== null && input.endTotal > 0
            ? value / input.endTotal
            : null,
        periodReturn: members.length > 0 ? input.periodReturnOf(tag.id) : null,
        gain: totals.gainLoss,
        gainPercent: totals.gainLossPercent,
        dividend: totals.dividend,
      });
    }
  }
  return rows;
}

/**
 * The positions the holdings table lists: the search narrows by company and ticker, and
 * the largest value at the end of the period comes first (unknown values last, equal values keep
 * their order).
 */
export function holdingRows(
  selected: readonly AnalysisPosition[],
  endValueOf: (positionId: string) => number | null,
  query: string
): AnalysisPosition[] {
  const value = (position: AnalysisPosition) => endValueOf(position.id) ?? -Infinity;
  return selected
    .filter((p) => matchesSearch(query, [p.companyName, p.ticker]))
    .map((position, order) => ({ position, order, value: value(position) }))
    .sort((a, b) => (a.value === b.value ? a.order - b.order : b.value - a.value))
    .map((entry) => entry.position);
}

/**
 * Concentration of the whole portfolio at the end of the period (positions without a known
 * value are not counted): the share of the five largest and the effective number of positions.
 */
export function portfolioConcentration(
  positions: readonly { id: string }[],
  endValueOf: (positionId: string) => number | null
): { top5Share: number; effective: number } | null {
  const values = positions.flatMap((p) => {
    const value = endValueOf(p.id);
    return value !== null && value > 0 ? [value] : [];
  });
  return values.length > 0 ? concentration(values) : null;
}
