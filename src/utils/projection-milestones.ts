/**
 * Milestones for the net-worth projection (design system §7 Outlook):
 * the point where the liabilities reach zero and the round net-worth marks
 * the expected path crosses on the way to the horizon.
 */

export interface ProjectionSeriesPoint {
  value: number;
  liabilities: number;
}

/** Index of the first point where the liabilities reach zero, if they start above it. */
export function payoffIndex(points: readonly ProjectionSeriesPoint[]): number | null {
  if (points.length === 0 || points[0].liabilities <= 0) return null;
  const i = points.findIndex((p) => p.liabilities <= 0);
  return i > 0 ? i : null;
}

/** 1 · 2 · 5 × 10^k marks strictly above `from` and at most `to`. */
export function roundMarks(from: number, to: number): number[] {
  if (!(to > from) || to <= 0) return [];
  const marks: number[] = [];
  for (let k = 3; k <= 12; k++) {
    for (const m of [1, 2, 5]) {
      const v = m * 10 ** k;
      if (v > from && v <= to) marks.push(v);
    }
  }
  return marks;
}

export interface RoundMilestone {
  value: number;
  /** Index of the first point at or above the value. */
  index: number;
}

/**
 * Up to `max` round marks crossed on the expected path, excluding the first
 * and the last point and any index already taken (the payoff). When more
 * marks qualify, the first, the middle and the last one are kept.
 */
export function roundMilestones(
  values: readonly number[],
  taken: readonly number[] = [],
  max = 3
): RoundMilestone[] {
  if (values.length < 3) return [];
  const first = values[0];
  const last = values[values.length - 1];
  const candidates: RoundMilestone[] = [];
  for (const value of roundMarks(first, last)) {
    const index = values.findIndex((v) => v >= value);
    if (index <= 0 || index >= values.length - 1) continue;
    if (taken.includes(index)) continue;
    // Several marks crossed at the same point: the largest one names it
    const existing = candidates.find((c) => c.index === index);
    if (existing) existing.value = Math.max(existing.value, value);
    else candidates.push({ value, index });
  }
  if (candidates.length <= max) return candidates;
  const picks = new Set<number>([
    0,
    Math.floor((candidates.length - 1) / 2),
    candidates.length - 1,
  ]);
  return candidates.filter((_, i) => picks.has(i)).slice(0, max);
}

/** Five evenly spaced axis labels: "today" and the years at each quarter of the horizon. */
export function horizonAxisLabels(
  startYear: number,
  years: number,
  todayLabel: string,
  count = 5
): string[] {
  const labels: string[] = [];
  for (let i = 0; i < count; i++) {
    labels.push(i === 0 ? todayLabel : String(startYear + Math.round((years * i) / (count - 1))));
  }
  return labels;
}
