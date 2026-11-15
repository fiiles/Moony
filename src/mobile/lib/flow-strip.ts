/**
 * The column width of the account's flows strip (README §13 `.m-flows`, audit D9): the strip shows
 * a whole number of months, so neither edge cuts a month or its name in half. As many columns of
 * at least `FLOW_COLUMN_MIN` as fit share the width evenly (never more than there are months); a
 * strip narrower than one column keeps the minimum. Pure.
 */

/** The narrowest month column: a 44 pt touch target. */
export const FLOW_COLUMN_MIN = 44;

export function flowColumnWidth(width: number, months: number): number {
  if (!Number.isFinite(width) || width < FLOW_COLUMN_MIN || months <= 0) return FLOW_COLUMN_MIN;
  const columns = Math.min(months, Math.floor(width / FLOW_COLUMN_MIN));
  return width / columns;
}
