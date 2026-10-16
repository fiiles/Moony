import type { StockTag } from '@shared/schema';
import { matchesSearch } from '@/utils/holdings-search';
import { concentration, tagGroupSeriesKey } from '@/utils/stock-performance';
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

/** What a row of the tags table sums over its positions. */
export interface TagFigures {
  /** Selected positions it counts. */
  count: number;
  /** Their value as of the end of the period; `null` when none is known. */
  value: number | null;
  /** Value as a share of the whole portfolio at the end, ratio. */
  share: number | null;
  /** Return of the positions over the period, percent; `null` without a series. */
  periodReturn: number | null;
  /** Money they gained over the period, in the report currency; `null` without a series. */
  periodGain: number | null;
  /** Unrealized gain as of today, in the report currency. */
  gain: number;
  /** Unrealized gain as of today, percent of cost. */
  gainPercent: number;
  /** Estimated dividend a year, as of today. */
  dividend: number;
}

/** A row of the tags table: the selected positions carrying the tag. */
export interface TagRow extends TagFigures {
  tag: StockTag;
  /** Key of its row of chips (`TagGroupRows.key`). */
  groupKey: string;
  /** Name of its group; `null` for a tag without one. */
  groupName: string | null;
}

/** The subtotal of a tag group: the selected positions carrying any of its tags. */
export interface TagGroupTotal extends TagFigures {
  groupKey: string;
  label: string;
}

/** The figures of a performance series over the whole period. */
export interface PeriodFigures {
  periodReturn: number | null;
  periodGain: number | null;
}

export interface TagRowsInput {
  groups: readonly TagGroupRows[];
  /** The positions the chips select. */
  selected: readonly AnalysisPosition[];
  /** Value of a position as of the end of the period. */
  endValueOf: (positionId: string) => number | null;
  /** Value of the whole portfolio as of the end of the period. */
  endTotal: number | null;
  /** A series of the performance response by key (`tag:<id>`, `tag-group:<id>`). */
  periodOf: (seriesKey: string) => PeriodFigures | null;
  /** With a filter only tags that have a selected position are listed. */
  filterActive: boolean;
}

function figuresOf(
  members: readonly AnalysisPosition[],
  seriesKey: string,
  input: TagRowsInput
): TagFigures {
  const totals = positionTotals(members);
  const value = sumKnown(members.map((p) => input.endValueOf(p.id)));
  const period = members.length > 0 ? input.periodOf(seriesKey) : null;
  return {
    count: members.length,
    value,
    share:
      value !== null && input.endTotal !== null && input.endTotal > 0
        ? value / input.endTotal
        : null,
    periodReturn: period?.periodReturn ?? null,
    periodGain: period?.periodGain ?? null,
    gain: totals.gainLoss,
    gainPercent: totals.gainLossPercent,
    dividend: totals.dividend,
  };
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
      rows.push({
        tag,
        groupKey: group.key,
        groupName: group.free ? null : group.label,
        ...figuresOf(members, `tag:${tag.id}`, input),
      });
    }
  }
  return rows;
}

/**
 * Subtotals of the tag groups, figured like a tag row over the selected positions carrying any
 * tag of the group (its period return is of the group's own series), so the rows of one group add
 * up to it. Only a group with at least two tags holding a selected position has one: with one it
 * would repeat that row. The tags without a group overlap and have none.
 */
export function tagGroupTotals(input: TagRowsInput): TagGroupTotal[] {
  return input.groups.flatMap((group) => {
    if (group.free) return [];
    const ids = new Set(group.tags.map((tag) => tag.id));
    const holding = group.tags.filter((tag) =>
      input.selected.some((p) => p.tags.some((x) => x.id === tag.id))
    );
    if (holding.length < 2) return [];
    const members = input.selected.filter((p) => p.tags.some((x) => ids.has(x.id)));
    return [
      {
        groupKey: group.key,
        label: group.label,
        ...figuresOf(members, tagGroupSeriesKey(group.key), input),
      },
    ];
  });
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
