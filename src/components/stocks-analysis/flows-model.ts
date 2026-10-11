import type { BucketKind, FlowBucket } from '@/utils/stock-performance';

/** The four cells above the bars: what the portfolio was worth, what came in, what the market did. */
export interface FlowBridge {
  start: number | null;
  /** Net deposits (buys minus sells) over the period. */
  deposits: number | null;
  /** The part of the change that is not a deposit. */
  market: number | null;
  end: number | null;
}

/**
 * The bridge from the first to the last point: `start + deposits + market = end`. A cell whose
 * inputs are unknown is `null`; `null` altogether when there are no points.
 */
export function flowBridge(
  values: readonly (number | null)[],
  netFlows: readonly (number | null)[]
): FlowBridge | null {
  if (values.length === 0) return null;
  const start = values[0];
  const end = values[values.length - 1];
  const deposits = netFlows[netFlows.length - 1] ?? null;
  const market =
    start !== null && end !== null && deposits !== null ? end - start - deposits : null;
  return { start, deposits, market, end };
}

/** What an axis label under a bucket says; the component writes it in the locale. */
export type FlowLabel =
  | { kind: 'week'; ts: number }
  | { kind: 'month'; ts: number; withYear: boolean }
  | { kind: 'quarter'; quarter: number; year: number }
  | { kind: 'year'; year: number };

/**
 * Label of a bucket: a week by its first day, a month by its name (with the two-digit year on
 * January and on the first bucket), a quarter by number and year, a year by the year.
 */
export function flowLabel(
  bucket: Pick<FlowBucket, 'start'>,
  kind: BucketKind,
  first: boolean
): FlowLabel {
  const date = new Date(bucket.start * 1000);
  const month = date.getUTCMonth();
  const year = date.getUTCFullYear();
  switch (kind) {
    case 'week':
      return { kind, ts: bucket.start };
    case 'month':
      return { kind, ts: bucket.start, withYear: month === 0 || first };
    case 'quarter':
      return { kind, quarter: Math.floor(month / 3) + 1, year };
    case 'year':
      return { kind, year };
  }
}
