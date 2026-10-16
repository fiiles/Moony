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
