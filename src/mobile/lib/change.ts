/** A value against the one it had at the start of a period. */
export interface PeriodChange {
  /** Today minus the base, in the value's own currency. */
  delta: number;
  /** `delta` against the magnitude of the base (0.021 = 2,1 %); `null` against a zero base. */
  ratio: number | null;
}

/**
 * Today's value against its base (the value a period ago). `null` when either is unknown: a class
 * that could not be valued today, or one with no recorded base. A negative base is measured by
 * its magnitude, so a debt that shrinks reads as a rise.
 */
export function periodChange(
  today: number | null,
  base: number | null | undefined
): PeriodChange | null {
  if (today === null || base === null || base === undefined) return null;
  const delta = today - base;
  return { delta, ratio: base === 0 ? null : delta / Math.abs(base) };
}

/**
 * Options for `fmt.day` for the day a change is really measured from when the history is shorter
 * than its period ("od 3. 10."): the same as Přehled's change label — month short, the year only
 * when the day is not in the current one.
 */
export function changeSinceDayOptions(
  unixSeconds: number,
  now = Date.now()
): Intl.DateTimeFormatOptions {
  const sameYear = new Date(unixSeconds * 1000).getUTCFullYear() === new Date(now).getUTCFullYear();
  return { month: 'short', day: 'numeric', year: sameYear ? undefined : 'numeric' };
}
