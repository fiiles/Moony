/**
 * Geometry of the scrubbable phone chart (spec §10.4: hover becomes scrub). Pure: the chart
 * measures its plot area and asks which point a finger at `x` is on. Coordinates are pixels of
 * the plot area (0 = its left edge), points sorted by `x` ascending.
 */

/**
 * Index of the point nearest to a finger at `x` on a plot `width` wide; `x` outside the plot is
 * clamped to it (beyond the end gives the last point), a tie keeps the earlier point, no points
 * gives `null`.
 */
export function nearestPoint(
  points: readonly { x: number }[],
  x: number,
  width: number
): number | null {
  if (points.length === 0) return null;
  const at = Math.max(0, Math.min(width, x));
  let lo = 0;
  let hi = points.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (points[mid].x < at) lo = mid + 1;
    else hi = mid;
  }
  if (lo > 0 && at - points[lo - 1].x <= points[lo].x - at) return lo - 1;
  return lo;
}

/** Horizontal pixel of time `t` on a plot `width` wide spanning `t0`…`t1` (a one-day span sits at the right edge). */
export function toPlotX(t: number, t0: number, t1: number, width: number): number {
  if (t1 <= t0) return width;
  return ((t - t0) / (t1 - t0)) * width;
}

/** Vertical pixel of `value` in the `domain` drawn `height` tall from `top` down (a flat domain is centred). */
export function toPlotY(
  value: number,
  domain: readonly [number, number],
  top: number,
  height: number
): number {
  const [lo, hi] = domain;
  if (hi <= lo) return top + height / 2;
  return top + ((hi - value) / (hi - lo)) * height;
}

/**
 * Value of a step line (a balance that changes only at a record) at time `t`: the latest point at
 * or before `t`; before the first point the first value, without points 0. Points sorted by `t`.
 */
export function stepValueAt(points: readonly { t: number; value: number }[], t: number): number {
  if (points.length === 0) return 0;
  let value = points[0].value;
  for (const point of points) {
    if (point.t > t) break;
    value = point.value;
  }
  return value;
}

/** A reading of a chart line; `cost` is the dashed cost basis some lines carry. */
interface Reading {
  t: number;
  value: number;
  cost?: number;
}

/** A reading with the line it is drawn on: `actual` up to the split, `projected` from it on. */
export interface SplitReading extends Reading {
  actual?: number;
  projected?: number;
}

/**
 * The rows of a line that continues as a plan (a loan's schedule after today): readings at or
 * before `projectedFrom` carry `actual`, readings at or after it carry `projected`, so the two lines
 * share the reading on the split. A split between two readings gets one of its own at the step
 * value of the reading before it (the phone draws a loan as a step line), so the lines meet there;
 * a split on a reading or outside the range adds nothing. Without `projectedFrom` the whole line is
 * `actual`. Every reading keeps its `value`, which the scrub reports whichever line it is on.
 */
export function splitProjected(
  points: readonly Reading[],
  projectedFrom: number | undefined
): SplitReading[] {
  if (projectedFrom === undefined) return points.map((p) => ({ ...p, actual: p.value }));
  const needsSplit =
    points.length > 1 &&
    projectedFrom > points[0].t &&
    projectedFrom < points[points.length - 1].t &&
    !points.some((p) => p.t === projectedFrom);
  const rows: Reading[] = needsSplit
    ? [...points, { t: projectedFrom, value: stepValueAt(points, projectedFrom) }].sort(
        (a, b) => a.t - b.t
      )
    : [...points];
  return rows.map((p) => ({
    ...p,
    actual: p.t <= projectedFrom ? p.value : undefined,
    projected: p.t >= projectedFrom ? p.value : undefined,
  }));
}
