/**
 * Pure helpers behind the chart wrappers (design system §8 and §9): nice
 * value ticks, the baseline rule, evenly spaced time-axis stamps and the
 * clustering of time-trace events. No DOM, no locale — labels are formatted
 * by the caller through useFormat().
 */

/** Smallest "nice" step (1 · 2 · 2.5 · 5 · 10 × 10ⁿ) that splits `span` into at most `n` parts. */
export function niceStep(span: number, n: number): number {
  if (!(span > 0) || !(n > 0)) return 1;
  const raw = span / n;
  const mag = 10 ** Math.floor(Math.log10(raw));
  return [1, 2, 2.5, 5, 10].map((m) => m * mag).find((v) => v >= raw) ?? 10 * mag;
}

/** Nice ticks inside [lo, hi], never more than ~n + 1. */
export function niceTicks(lo: number, hi: number, n = 4): number[] {
  if (!(hi > lo)) return [lo];
  const step = niceStep(hi - lo, n);
  const ticks: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi + step * 1e-9; v += step) {
    // Avoid -0 and float noise ("1.0000000002")
    ticks.push(Math.abs(v) < step * 1e-9 ? 0 : Number(v.toPrecision(12)));
  }
  return ticks;
}

export interface DomainOptions {
  /** Zero baseline ("Vše" and bars); otherwise the baseline is truncated with headroom below. */
  zeroBaseline?: boolean;
  /** Fraction of the span added below the minimum when the baseline is truncated (README §8: 15 %). */
  headroomBelow?: number;
  /** Fraction of the span added above the maximum. */
  headroomAbove?: number;
}

/**
 * Value domain for a line chart. Truncated baselines never dip below zero for
 * non-negative data, and a flat series still gets a visible band around it.
 */
export function valueDomain(values: readonly number[], opts: DomainOptions = {}): [number, number] {
  const { zeroBaseline = false, headroomBelow = 0.15, headroomAbove = 0.08 } = opts;
  const finite = values.filter((v) => Number.isFinite(v));
  if (finite.length === 0) return [0, 1];
  let lo = Math.min(...finite);
  const hi = Math.max(...finite);
  if (zeroBaseline) lo = Math.min(0, lo);
  let span = hi - lo;
  if (span === 0) span = Math.abs(hi) || 1;
  const below = zeroBaseline && lo === 0 ? 0 : span * headroomBelow;
  const bottom = lo - below;
  // Non-negative data never shows a negative baseline
  const floor = lo >= 0 ? Math.max(0, bottom) : bottom;
  return [floor, hi + span * headroomAbove];
}

/** `n` evenly spaced stamps from `t0` to `t1` inclusive (unix seconds), for the five axis labels. */
export function axisStamps(t0: number, t1: number, n = 5): number[] {
  if (n <= 1 || t1 <= t0) return [t1];
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(Math.round(t0 + ((t1 - t0) * i) / (n - 1)));
  return out;
}

/** Spans under this many days label the axis with "2. 10."; longer ones with "zář 26". */
export const SHORT_SPAN_DAYS = 100;

export function isShortSpan(t0: number, t1: number): boolean {
  return t1 - t0 < SHORT_SPAN_DAYS * 86400;
}

/** Index of the point whose `t` is nearest to `t` (points sorted ascending). */
export function nearestIndex(points: readonly { t: number }[], t: number): number {
  if (points.length === 0) return -1;
  let lo = 0;
  let hi = points.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (points[mid].t < t) lo = mid + 1;
    else hi = mid;
  }
  if (lo > 0 && Math.abs(points[lo - 1].t - t) <= Math.abs(points[lo].t - t)) return lo - 1;
  return lo;
}

/** Linear interpolation of the series value at `t` (clamped to the ends). */
export function valueAt(points: readonly { t: number; value: number }[], t: number): number {
  if (points.length === 0) return 0;
  if (t <= points[0].t) return points[0].value;
  const last = points[points.length - 1];
  if (t >= last.t) return last.value;
  const i = nearestIndex(points, t);
  const a = points[i].t <= t ? points[i] : points[i - 1];
  const b = points[i].t <= t ? points[i + 1] : points[i];
  if (!a || !b || b.t === a.t) return (a ?? b).value;
  return a.value + ((b.value - a.value) * (t - a.t)) / (b.t - a.t);
}

/** `mark` is the neutral ring (a revaluation, a contract change) that carries no direction. */
export type ChartEventType = 'buy' | 'sell' | 'income' | 'expense' | 'mark';

export interface ChartEvent {
  id: string;
  /** Unix seconds. */
  t: number;
  type: ChartEventType;
}

export interface EventCluster<E extends ChartEvent = ChartEvent> {
  /** Id of the first event (stable key, scroll target). */
  id: string;
  t: number;
  /** The most frequent type inside the cluster. */
  type: ChartEventType;
  events: E[];
}

/** Clusters grow only when more than this many events would be drawn (README §9: ~24). */
export const MAX_VISIBLE_EVENTS = 24;

/**
 * Events inside [t0, t1], one mark each while there are at most `max` of
 * them; above that, neighbours falling into the same of `max` equal time
 * buckets fold into one mark that lists them.
 */
export function clusterEvents<E extends ChartEvent>(
  events: readonly E[],
  t0: number,
  t1: number,
  max = MAX_VISIBLE_EVENTS
): EventCluster<E>[] {
  const inView = events.filter((e) => e.t >= t0 && e.t <= t1).sort((a, b) => a.t - b.t);
  if (inView.length <= max || t1 <= t0) {
    return inView.map((e) => ({ id: e.id, t: e.t, type: e.type, events: [e] }));
  }
  const buckets = new Map<number, E[]>();
  const width = (t1 - t0) / max;
  for (const e of inView) {
    const k = Math.min(max - 1, Math.floor((e.t - t0) / width));
    const list = buckets.get(k);
    if (list) list.push(e);
    else buckets.set(k, [e]);
  }
  return [...buckets.values()].map((list) => {
    const counts = new Map<ChartEventType, number>();
    for (const e of list) counts.set(e.type, (counts.get(e.type) ?? 0) + 1);
    const type = [...counts.entries()].sort((a, b) => b[1] - a[1])[0][0];
    const t = Math.round(list.reduce((s, e) => s + e.t, 0) / list.length);
    return { id: list[0].id, t, type, events: list };
  });
}
