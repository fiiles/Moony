/**
 * Ordering for converted amounts: an amount that could not be converted (`null`) sorts
 * below every known one, so a descending column ends with the rows without a rate.
 */
export function compareConverted(a: number | null, b: number | null): number {
  if (a === b) return 0;
  if (a === null) return -1;
  if (b === null) return 1;
  return a - b;
}
