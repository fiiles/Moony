/**
 * Trading-time axis for intraday price charts: nights, weekends and lunch breaks
 * (the hours an exchange does not trade) shrink to one bar, so the sessions sit
 * next to each other instead of being joined by a long flat line.
 */

/** A pause between two points longer than this many usual bar spacings is a closed market. */
const GAP_FACTOR = 3;

/** The usual spacing of the series: the median of the steps between its points. */
function usualStep(times: readonly number[]): number | null {
  const steps: number[] = [];
  for (let i = 1; i < times.length; i++) {
    const d = times[i] - times[i - 1];
    if (d > 0) steps.push(d);
  }
  if (steps.length === 0) return null;
  steps.sort((a, b) => a - b);
  return steps[Math.floor(steps.length / 2)];
}

function isGap(step: number, usual: number): boolean {
  return step > usual * GAP_FACTOR;
}

export interface TradingAxis {
  /** Axis position (seconds of trading time, the first point at its own time) of a real time. */
  toAxis: (t: number) => number;
  /** Real Unix seconds of an axis position: the exact inverse of `toAxis`. */
  fromAxis: (x: number) => number;
}

/**
 * Axis over the ascending `times` where every pause longer than three usual steps
 * takes one usual step. Piecewise linear between the points (a time inside a
 * shrunk pause lands proportionally inside its one step) and linear beyond them.
 */
export function tradingAxis(times: readonly number[]): TradingAxis {
  const usual = usualStep(times);
  if (usual === null) return { toAxis: (t) => t, fromAxis: (x) => x };
  const xs: number[] = [times[0]];
  for (let i = 1; i < times.length; i++) {
    const d = times[i] - times[i - 1];
    xs.push(xs[i - 1] + (isGap(d, usual) ? usual : d));
  }
  const last = times.length - 1;

  const toAxis = (t: number) => {
    if (t <= times[0]) return xs[0] + (t - times[0]);
    if (t >= times[last]) return xs[last] + (t - times[last]);
    let i = 1;
    while (times[i] < t) i++;
    const span = times[i] - times[i - 1];
    if (span <= 0) return xs[i];
    return xs[i - 1] + ((t - times[i - 1]) / span) * (xs[i] - xs[i - 1]);
  };
  const fromAxis = (x: number) => {
    if (x <= xs[0]) return times[0] + (x - xs[0]);
    if (x >= xs[last]) return times[last] + (x - xs[last]);
    let i = 1;
    while (xs[i] < x) i++;
    const span = xs[i] - xs[i - 1];
    if (span <= 0) return times[i];
    return times[i - 1] + ((x - xs[i - 1]) / span) * (times[i] - times[i - 1]);
  };
  return { toAxis, fromAxis };
}

/** Indexes of the first point of every trading session (the first point, then each after a pause). */
export function sessionStarts(times: readonly number[]): number[] {
  if (times.length === 0) return [];
  const usual = usualStep(times);
  const starts = [0];
  if (usual === null) return starts;
  for (let i = 1; i < times.length; i++) {
    if (isGap(times[i] - times[i - 1], usual)) starts.push(i);
  }
  return starts;
}

/**
 * The series ending on the live price fetched at `at`. Fetched during the session
 * (within three usual steps of the last bar) it is a new point; fetched later
 * (the market has closed since) or before the last bar, the last bar takes the
 * price, so the chart ends on it without a flat line across the closed hours.
 */
export function withLivePrice<P extends { t: number; value: number }>(
  points: readonly P[],
  price: number,
  at: number
): P[] {
  const pts = points.map((p) => ({ ...p }));
  if (!isFinite(price) || pts.length === 0) return pts;
  const last = pts[pts.length - 1];
  const usual = usualStep(pts.map((p) => p.t));
  if (at > last.t && (usual === null || !isGap(at - last.t, usual))) {
    pts.push({ ...last, t: at, value: price });
  } else {
    last.value = price;
  }
  return pts;
}
