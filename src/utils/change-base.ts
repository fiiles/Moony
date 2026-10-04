const DAY = 86_400;

/**
 * How much later than the period start the oldest history row may be and still
 * count as "the history reaches back to the start": rows are daily, but the
 * first one after an arbitrary start time can be a day or so later, and a missed
 * day is not worth a different label.
 */
export const SHORT_HISTORY_TOLERANCE_SECONDS = 2 * DAY;

/**
 * The day (midnight UTC, ADR 0008) a "change over <period>" is really measured
 * from when the history is shorter than the period — e.g. 10 days of history
 * behind a "30D" card — or `null` when the history covers the whole period (or
 * there is nothing to compare). The card then says "vs Sep 20" instead of
 * "vs 30 days ago".
 *
 * @param periodStart start of the requested period (unix seconds); `undefined`
 *   for "All", which is labelled "since first record" anyway
 * @param oldestRecordedAt `recordedAt` of the oldest history row in range
 */
export function shortHistoryBaseDay(
  periodStart: number | undefined,
  oldestRecordedAt: number | undefined
): number | null {
  if (periodStart === undefined || oldestRecordedAt === undefined) return null;
  if (oldestRecordedAt - periodStart <= SHORT_HISTORY_TOLERANCE_SECONDS) return null;
  return Math.floor(oldestRecordedAt / DAY) * DAY;
}
