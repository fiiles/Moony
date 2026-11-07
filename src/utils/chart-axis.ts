/**
 * Locale-independent helpers for time-series chart axes.
 * Labels are produced by the formatting module (`day()` / `month()`); this
 * file only decides which labels get a tick and how a day is labelled.
 */

/** At most this many labels sit on a time axis, however long the series is. */
export const MAX_AXIS_TICKS = 8;

/** Chart periods whose span reaches past one calendar year (labels need the year). */
const MULTI_YEAR_PERIODS = new Set(['1Y', '5Y', 'All']);

/**
 * Evenly spaced picks from `values` (first and last always included, so the
 * axis starts and ends on real points). Equal neighbours are collapsed because
 * Recharts matches category ticks by value.
 */
export function evenTicks<T>(values: readonly T[], max: number = MAX_AXIS_TICKS): T[] {
  const n = values.length;
  if (n === 0) return [];
  if (max < 2 || n === 1) return [values[0]];
  const count = Math.min(n, max);
  const picked: T[] = [];
  for (let i = 0; i < count; i++) {
    const value = values[Math.round((i * (n - 1)) / (count - 1))];
    if (picked.length === 0 || picked[picked.length - 1] !== value) picked.push(value);
  }
  return picked;
}

/** Options for a day label on an axis or tooltip: "Sep 30" / "Sep 30, 25" (year for 1Y and longer). */
export function dayLabelOptions(period: string): Intl.DateTimeFormatOptions {
  return MULTI_YEAR_PERIODS.has(period)
    ? { month: 'short', day: 'numeric', year: '2-digit' }
    : { month: 'short', day: 'numeric' };
}

/** Start of the UTC calendar day containing `unixSeconds` (ADR 0008: days are UTC). */
export function utcDayFloor(unixSeconds: number): number {
  return Math.floor(unixSeconds / 86400) * 86400;
}

/**
 * Labels for the value ticks of an axis, none repeated: a compact number with one decimal prints
 * the ticks "11,2 mil." and "11,2 mil." for 11,21 and 11,24 million (the steps of `niceTicks` are
 * rounder than the domain's offset, not than the label). `format(value, digits)` writes a value
 * with at most `digits` decimals; the fewest digits (from 1) that tell every tick apart are used
 * for the whole axis, so the labels stay alike, up to `maxDigits` (then the last try stands:
 * equal labels for ticks that close are the formatter's own limit).
 */
export function distinctTickLabels(
  ticks: readonly number[],
  format: (value: number, digits: number) => string,
  maxDigits = 3
): Map<number, string> {
  const values = [...new Set(ticks)];
  let labels = new Map<number, string>();
  for (let digits = 1; digits <= maxDigits; digits++) {
    labels = new Map(values.map((value) => [value, format(value, digits)]));
    if (new Set(labels.values()).size === values.length) break;
  }
  return labels;
}
