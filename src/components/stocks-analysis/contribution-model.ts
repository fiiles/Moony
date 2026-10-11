import type { PositionPerformance } from '@shared/schema';
import { splitTop } from '@/utils/stock-performance';
import type { RankedPart } from './performance-model';

/** One row of the contribution card: a position, a part of the chosen group or the rest. */
export interface ContributionItem {
  id: string;
  name: string;
  /** Ticker of a position: it gets a logo and no sub-line. */
  ticker: string | null;
  /** Positions a part or the rest row stands for; `null` for a position. */
  positions: number | null;
  /** Percentage points of the whole-portfolio return it brought over the period. */
  points: number;
  /** Return of the item itself over the period, percent; `null` for the rest row. */
  periodReturn: number | null;
  /** Quiet row: the part without a tag or sector, a part the chart does not draw, the rest. */
  muted: boolean;
}

/** How the rows are ordered: positions by the size of their effect, parts best first. */
export type ContributionOrder = 'magnitude' | 'value';

/**
 * Rows ordered, then folded: with more than nine rows the eight largest effects stay and the
 * others become one muted "rest" row that sums them (`otherName`), at the end.
 */
export function contributionRows(
  items: readonly ContributionItem[],
  order: ContributionOrder,
  otherName: string
): ContributionItem[] {
  const sorted = [...items].sort((a, b) =>
    order === 'magnitude' ? Math.abs(b.points) - Math.abs(a.points) : b.points - a.points
  );
  const { shown, rest } = splitTop(sorted, (item) => item.points);
  if (rest.length === 0) return shown;
  return [
    ...shown,
    {
      id: 'other',
      name: otherName,
      ticker: null,
      positions: rest.reduce((sum, item) => sum + (item.positions ?? 1), 0),
      points: rest.reduce((sum, item) => sum + item.points, 0),
      periodReturn: null,
      muted: true,
    },
  ];
}

/** Position of the zero line and of every bar, as percentages of the track width. */
export interface BarLayout {
  zero: number;
  bars: { left: number; width: number }[];
}

/** Narrowest bar, so a value that rounds to nothing still leaves a mark. */
const MIN_BAR = 0.6;

/**
 * Layout of a diverging bar list: one shared scale from the largest negative to the largest
 * positive value, the zero line where the negative side ends, positive bars growing right of it
 * and negative bars left of it.
 */
export function barLayout(values: readonly number[]): BarLayout {
  const maxPositive = Math.max(0, ...values);
  const maxNegative = Math.max(0, ...values.map((v) => -v));
  const span = maxPositive + maxNegative || 1;
  const zero = (maxNegative / span) * 100;
  return {
    zero,
    bars: values.map((value) => {
      const width = (Math.abs(value) / span) * 100;
      return { left: value >= 0 ? zero : zero - width, width: Math.max(MIN_BAR, width) };
    }),
  };
}

/** What a position needs to be listed in the contribution card. */
export interface ContributionPosition {
  id: string;
  companyName: string;
  ticker: string;
}

/**
 * Items of the "Pozice" mode: the selected positions the response has figures for (a position
 * the previous response does not know is left out), unordered.
 */
export function positionItems(
  selected: readonly ContributionPosition[],
  performance: ReadonlyMap<string, Pick<PositionPerformance, 'contribution' | 'twr'>>
): ContributionItem[] {
  return selected.flatMap((position) => {
    const figures = performance.get(position.id);
    if (!figures) return [];
    return [
      {
        id: position.id,
        name: position.companyName,
        ticker: position.ticker,
        positions: null,
        points: figures.contribution,
        periodReturn: figures.twr,
        muted: false,
      },
    ];
  });
}

/**
 * Items of the "Podle skupiny" mode: the parts of the chosen group, last point of their series
 * (`contribution` and `twr` of the requested series, by key). A part whose series is missing, or
 * has no contribution, is left out. Parts without a tag or sector and parts the chart does not
 * draw are muted.
 */
export function partItems(
  parts: readonly RankedPart[],
  series: ReadonlyMap<string, { twr: readonly number[]; contribution: readonly number[] | null }>,
  nameOf: (part: RankedPart['part']) => string
): ContributionItem[] {
  return parts.flatMap(({ part, drawn }) => {
    const found = series.get(part.key);
    const points = found?.contribution?.at(-1);
    if (!found || points === undefined) return [];
    return [
      {
        id: part.key,
        name: nameOf(part),
        ticker: null,
        positions: part.positionIds.length,
        points,
        periodReturn: found.twr.at(-1) ?? null,
        muted: part.isNone || !drawn,
      },
    ];
  });
}
