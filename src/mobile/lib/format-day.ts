/**
 * Options for `fmt.day` that write a day short ("10. 10."), with the year when it is not this
 * one ("10. 10. 2025"): the dates of the phone's sub-lines, where a year would cost the name its room.
 */
export function shortDayOptions(unixSeconds: number, now = Date.now()): Intl.DateTimeFormatOptions {
  const sameYear = new Date(unixSeconds * 1000).getUTCFullYear() === new Date(now).getUTCFullYear();
  return { day: 'numeric', month: 'numeric', year: sameYear ? undefined : 'numeric' };
}
