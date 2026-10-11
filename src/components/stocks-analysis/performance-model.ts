import type { PerformanceSeries, StockPerformance, StockTag } from '@shared/schema';
import { drawdown, type Part } from '@/utils/stock-performance';
import type { PerformanceLine } from './PerformanceChart';

/** The chart draws at most this many parts; the rest are listed in the board only. */
export const MAX_DRAWN = 4;
const PART_COLORS = ['var(--s1)', 'var(--s2)', 'var(--s3)', 'var(--s4)'] as const;

export type IndexId = 'sp500' | 'msci';

/** Indices offered for comparison and the ETF each is read through (Yahoo tickers). */
export const INDEX_CHOICES: readonly { id: IndexId; name: string; ticker: string }[] = [
  { id: 'sp500', name: 'S&P 500', ticker: 'CSPX.L' },
  { id: 'msci', name: 'MSCI World', ticker: 'IWDA.L' },
];

/** Decimal TEXT of the wire to a number; `null` for missing or unreadable amounts. */
export function amountOf(text: string | null | undefined): number | null {
  if (text == null) return null;
  const value = Number(text);
  return Number.isFinite(value) ? value : null;
}

/** Sum of the known amounts; `null` when none is known (unknown is not 0). */
export function sumKnown(values: readonly (number | null)[]): number | null {
  let sum = 0;
  let known = false;
  for (const value of values) {
    if (value === null) continue;
    sum += value;
    known = true;
  }
  return known ? sum : null;
}

/** A part of the chosen group with its value as of the end of the period and how it is drawn. */
export interface RankedPart {
  part: Part;
  /** Sum of the known values of its positions as of the end; `null` when none is known. */
  value: number | null;
  /** One of the four largest: it has a line in the chart. */
  drawn: boolean;
  /** `var(--s1)` … `var(--s4)` for a drawn part. */
  color: string | null;
}

/**
 * Parts by value, largest first, the part without a tag or sector last; the first four (never the
 * one without a tag or sector) are drawn in `s1`…`s4`. Equal values keep the order of `parts`.
 */
export function rankParts(
  parts: readonly Part[],
  valueOf: (positionId: string) => number | null
): RankedPart[] {
  const valued = parts.map((part) => ({ part, value: sumKnown(part.positionIds.map(valueOf)) }));
  const regular = valued
    .filter((entry) => !entry.part.isNone)
    .sort((a, b) => (b.value ?? -Infinity) - (a.value ?? -Infinity));
  const none = valued.filter((entry) => entry.part.isNone);
  return [...regular, ...none].map((entry, i) => {
    const drawn = !entry.part.isNone && i < MAX_DRAWN;
    return { ...entry, drawn, color: drawn ? PART_COLORS[i] : null };
  });
}

/** One row of the board: a part, the whole portfolio or the index. */
export interface ModelRow {
  /** Also the id of its chart line. */
  id: string;
  kind: 'part' | 'portfolio' | 'index';
  name: string;
  tag: StockTag | null;
  /** Quote currency of a currency part. */
  currency: string | null;
  /** The part without a tag or sector. */
  isNone: boolean;
  /** The row has a line in the chart. */
  drawn: boolean;
  /** CSS colour of a drawn part. */
  color: string | null;
  /** Muted row: the part without a tag or sector, or a part that is not drawn. */
  quiet: boolean;
  /** Cumulative return in percent per date. */
  twr: (number | null)[];
  drawdown: (number | null)[];
  /** Percentage points of the portfolio return per date; `null` where the backend has none. */
  contribution: number[] | null;
  /** Return in the part's own currency per date (a currency part in a foreign currency). */
  native: number[] | null;
  count: number | null;
  /** Value as of the end of the period. */
  value: number | null;
  /** Share of the selection's value, as a ratio. */
  share: number | null;
  /** The ETF an index is read through. */
  ticker: string | null;
}

export interface BuildRowsInput {
  series: readonly PerformanceSeries[];
  portfolio: readonly number[];
  ranked: readonly RankedPart[];
  nameOf: (part: Part) => string;
  portfolioName: string;
  /** Value of the whole portfolio as of the end. */
  totalValue: number | null;
  /** Value of the selected positions as of the end. */
  selectedValue: number | null;
  filterActive: boolean;
  index: { name: string; ticker: string; twr: (number | null)[] } | null;
  reportCurrency: string;
}

/**
 * Rows of the board in display order: the parts by value, the portfolio, the index, the part
 * without a tag or sector. A part whose series the response does not carry (the previous request's
 * data is still on screen) is left out.
 */
export function buildRows(input: BuildRowsInput): ModelRow[] {
  const byKey = new Map(input.series.map((s) => [s.key, s]));
  const share = (value: number | null) =>
    value !== null && input.selectedValue !== null && input.selectedValue > 0
      ? value / input.selectedValue
      : null;

  const parts: ModelRow[] = [];
  const none: ModelRow[] = [];
  for (const { part, value, drawn, color } of input.ranked) {
    const series = byKey.get(part.key);
    if (!series) continue;
    const foreign = part.currency !== null && part.currency !== input.reportCurrency;
    const row: ModelRow = {
      id: part.key,
      kind: 'part',
      name: input.nameOf(part),
      tag: part.tag,
      currency: part.currency,
      isNone: part.isNone,
      drawn,
      color,
      quiet: part.isNone || !drawn,
      twr: series.twr,
      drawdown: drawdown(series.twr),
      contribution: series.contribution,
      native: foreign ? (byKey.get(`${part.key}:native`)?.twr ?? null) : null,
      count: part.positionIds.length,
      value,
      share: share(value),
      ticker: null,
    };
    (part.isNone ? none : parts).push(row);
  }

  const portfolio: ModelRow = {
    id: 'portfolio',
    kind: 'portfolio',
    name: input.portfolioName,
    tag: null,
    currency: null,
    isNone: false,
    drawn: true,
    color: 'var(--chart-line)',
    quiet: false,
    twr: [...input.portfolio],
    drawdown: drawdown(input.portfolio),
    contribution: null,
    native: null,
    count: null,
    value: input.totalValue,
    share: input.filterActive ? null : input.totalValue === null ? null : 1,
    ticker: null,
  };
  const index: ModelRow[] = input.index
    ? [
        {
          id: 'index',
          kind: 'index',
          name: input.index.name,
          tag: null,
          currency: null,
          isNone: false,
          drawn: true,
          color: 'var(--ink-4)',
          quiet: false,
          twr: input.index.twr,
          drawdown: drawdown(input.index.twr),
          contribution: null,
          native: null,
          count: null,
          value: null,
          share: null,
          ticker: input.index.ticker,
        },
      ]
    : [];
  return [...parts, portfolio, ...index, ...none];
}

/** The chart's lines: the drawn parts, then the index, then the portfolio as the reference. */
export function chartLines(rows: readonly ModelRow[]): PerformanceLine[] {
  const line = (row: ModelRow, variant: PerformanceLine['variant']): PerformanceLine => ({
    id: row.id,
    name: row.name,
    color: row.color ?? 'var(--s-other)',
    variant,
    twr: row.twr,
    drawdown: row.drawdown,
  });
  return [
    ...rows.filter((r) => r.kind === 'part' && r.drawn).map((r) => line(r, 'solid')),
    ...rows.filter((r) => r.kind === 'index').map((r) => line(r, 'index')),
    ...rows.filter((r) => r.kind === 'portfolio').map((r) => line(r, 'reference')),
  ];
}

/**
 * The drawn parts that have a value at point `index`, best first. The hero names the first and the
 * last of them.
 */
export function rankedAt(
  rows: readonly ModelRow[],
  index: number
): { row: ModelRow; twr: number }[] {
  const out: { row: ModelRow; twr: number }[] = [];
  for (const row of rows) {
    const twr = row.twr[index];
    if (row.kind === 'part' && row.drawn && twr !== null && twr !== undefined) {
      out.push({ row, twr });
    }
  }
  return out.sort((a, b) => b.twr - a.twr);
}

/** The three hero figures at one point of the period. */
export interface Figures {
  /** Time-weighted return of the whole portfolio, percent. */
  twr: number;
  /** Money-weighted return, percent. */
  mwr: number;
  /** `mwr − twr` in percentage points. */
  mwrGap: number;
  /** Change of value since the first date; `null` when a value is not convertible. */
  change: number | null;
  /** Net deposits since the first date. */
  deposits: number | null;
  /** Market effect: the change that is not a deposit. */
  market: number | null;
}

/** The figures at point `index`, or `null` when the response has no such point. */
export function figuresAt(
  performance: Pick<StockPerformance, 'portfolio' | 'mwr' | 'values' | 'netFlows'>,
  index: number
): Figures | null {
  const twr = performance.portfolio[index];
  if (twr === undefined) return null;
  const mwr = performance.mwr[index] ?? 0;
  const start = amountOf(performance.values[0]);
  const now = amountOf(performance.values[index]);
  const deposits = amountOf(performance.netFlows[index]);
  const change = start !== null && now !== null ? now - start : null;
  return {
    twr,
    mwr,
    mwrGap: mwr - twr,
    change,
    deposits,
    market: change !== null && deposits !== null ? change - deposits : null,
  };
}

/** Exchange rate effect of a currency part in percentage points: report-currency minus native. */
export function fxEffect(row: Pick<ModelRow, 'twr' | 'native'>, index: number): number | null {
  const reported = row.twr[index];
  const native = row.native?.[index];
  if (reported === null || reported === undefined || native === undefined) return null;
  return reported - native;
}
