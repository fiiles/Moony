/**
 * The value trace of a manually priced asset (design system §9, the real estate
 * detail): purchase → every estimate → today, as the points of a line chart.
 *
 * Pure: no clock, no locale. The caller converts the values to the display
 * currency first (`convertValuations`: every estimate at its own day) and passes
 * `now`. Days are UTC-midnight unix seconds (ADR 0008).
 */

import type { ConvertAtFn } from '@shared/fx';

const DAY = 86_400;

export interface TraceValuation {
  id: string;
  /** UTC day the estimate applies to. */
  t: number;
  value: number;
  /** When the row was entered; orders estimates of one day. */
  createdAt: number;
}

export interface TracePoint {
  t: number;
  value: number;
}

export interface ValuationTraceInput {
  valuations: readonly TraceValuation[];
  /** What the property cost, and the day it was bought; `null` while the day is unknown. */
  purchase?: TracePoint | null;
  /** Unix seconds; the line runs up to here. */
  now: number;
  /**
   * The value at `now` (the latest estimate at today's rates); defaults to the
   * last estimate's own value.
   */
  nowValue?: number;
  /** Unix seconds; the trace shows nothing earlier than this. */
  start?: number;
}

/**
 * Estimates ordered by day, then by creation. Of several rows on one day only
 * the last one stays: the day's value is the newest estimate. The rows are
 * returned as given, so callers can read their own fields back.
 */
export function dailyValuations<V extends { t: number; createdAt: number }>(
  valuations: readonly V[]
): V[] {
  const sorted = [...valuations].sort((a, b) => a.t - b.t || a.createdAt - b.createdAt);
  const days: V[] = [];
  for (const row of sorted) {
    if (days.length > 0 && days[days.length - 1].t === row.t) days[days.length - 1] = row;
    else days.push(row);
  }
  return days;
}

/**
 * Points of the value line. The purchase opens the line when its day precedes
 * the first estimate; a value holds until the day before it changes, so every
 * revaluation is a step, not a slope across the gap; the last value runs on up
 * to `now`. With `start`, everything before it is replaced by one anchor
 * carrying the value that applied then. A property without an estimate has no
 * trace. The days are strictly increasing.
 */
export function valuationTrace({
  valuations,
  purchase,
  now,
  nowValue,
  start,
}: ValuationTraceInput): TracePoint[] {
  const days = dailyValuations(valuations);
  if (days.length === 0) return [];

  const points: TracePoint[] = [];
  // Every point goes through here, so the days can never repeat or run backwards.
  const push = (t: number, value: number) => {
    const last = points[points.length - 1];
    if (last === undefined || t > last.t) points.push({ t, value });
  };

  let previous: number | undefined;
  if (purchase && purchase.t < days[0].t) {
    push(purchase.t, purchase.value);
    previous = purchase.value;
  }
  for (const day of days) {
    if (previous !== undefined) push(day.t - DAY, previous);
    push(day.t, day.value);
    previous = day.value;
  }
  push(now, nowValue ?? days[days.length - 1].value);

  if (start === undefined) return points;
  const before = points.filter((p) => p.t < start);
  const inside = points.filter((p) => p.t >= start);
  if (before.length === 0 || inside[0]?.t === start) return inside;
  return [{ t: start, value: before[before.length - 1].value }, ...inside];
}

/** A stored estimate in its own currency. */
export interface NativeValuation {
  id: string;
  /** UTC day the estimate applies to. */
  t: number;
  value: number;
  currency: string;
  createdAt: number;
}

/**
 * Estimates in the display currency, each at the rates of its own day. An estimate whose
 * day cannot be converted is left off the trace (the line bridges it) and its currency
 * reported in `missing`. Extra fields of the rows are kept.
 */
export function convertValuations<V extends NativeValuation>(
  valuations: readonly V[],
  convertAt: ConvertAtFn
): { valuations: (Omit<V, 'value' | 'currency'> & { value: number })[]; missing: string[] } {
  const missing = new Set<string>();
  const out: (Omit<V, 'value' | 'currency'> & { value: number })[] = [];
  for (const row of valuations) {
    const { currency, value, ...rest } = row;
    const converted = value === 0 ? 0 : convertAt(value, currency, row.t);
    if (converted === null) missing.add(currency);
    else out.push({ ...rest, value: converted });
  }
  return { valuations: out, missing: [...missing] };
}
