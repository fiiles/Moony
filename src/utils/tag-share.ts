/**
 * Rows for the Stocks Analysis "share of portfolio by tag" bars: one row per tag,
 * largest value first, with the bar length scaled to the largest tag so the smaller ones stay
 * visible. The percentage label is the tag's real share of the portfolio (a holding can carry
 * several tags, so the shares may add up to more than 100 %).
 */

import type { TagMetrics } from '@shared/schema';

export interface TagShareRow {
  id: string;
  name: string;
  color: string | null;
  /** Total value of the tag's holdings, in the response's report currency */
  value: number;
  /** Share of the whole portfolio, 0-100 */
  percent: number;
  /** Bar length, 0-100, relative to the largest tag */
  barPercent: number;
}

export function buildTagShareRows(metrics: readonly TagMetrics[]): TagShareRow[] {
  const valueOf = (m: TagMetrics) => Number(m.value) || 0;
  const sorted = [...metrics].sort(
    (a, b) => valueOf(b) - valueOf(a) || a.tag.name.localeCompare(b.tag.name)
  );
  const max = sorted.reduce((largest, m) => Math.max(largest, valueOf(m)), 0);

  return sorted.map((m) => ({
    id: m.tag.id,
    name: m.tag.name,
    color: m.tag.color,
    value: valueOf(m),
    percent: m.portfolioPercent,
    barPercent: max > 0 ? (valueOf(m) / max) * 100 : 0,
  }));
}
