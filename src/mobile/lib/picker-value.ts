/**
 * What the phone's date and month fields show over their native pickers (audit B8): the ISO value
 * the input speaks as the UTC instant the app's formatter writes (`fmt.day`, `fmt.month`), or
 * `null` for an empty or unreadable value, which the field shows as a word. Pure.
 */

/** `YYYY-MM-DD` as the second of its UTC midnight; `null` when it is not a real day. */
export function isoDayToUtcSeconds(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const date = new Date(Date.UTC(year, month - 1, day));
  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day
  ) {
    return null;
  }
  return date.getTime() / 1000;
}

/** `YYYY-MM` as the UTC date of the month's first day; `null` when it is not a month. */
export function isoMonthToUtcDate(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})$/.exec(value);
  if (!match) return null;
  const month = Number(match[2]);
  if (month < 1 || month > 12) return null;
  return new Date(Date.UTC(Number(match[1]), month - 1, 1));
}
