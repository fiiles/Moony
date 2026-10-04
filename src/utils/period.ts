/**
 * Calendar periods in UTC.
 *
 * Moony stores every day-granular date (bank transaction booking dates, CSV
 * imports, manual entries, investment transactions) as the UTC midnight epoch
 * of that calendar day (ADR 0008). Period boundaries must therefore be
 * computed in UTC as well — computing them in local time drops the last day of
 * every month/quarter/year for users east of UTC and the first day for users
 * west of it.
 *
 * All timestamps are unix seconds. `end` is inclusive (23:59:59 UTC of the
 * last day), matching the backend's `booking_date <= end` filters.
 */

export type PeriodTimeframe = 'monthly' | 'quarterly' | 'yearly';

export interface UtcPeriodRange {
  /** First second of the period (UTC midnight of its first day). */
  start: number;
  /** Last second of the period (23:59:59 UTC of its last day). */
  end: number;
  /** Same instant as `start`, for UTC-aware label formatting. */
  startDate: Date;
  /** UTC midnight of the last day of the period, for labels. */
  endDate: Date;
}

const SECOND = 1000;

/**
 * Period containing `now` shifted by `offset` periods (0 = current,
 * -1 = previous, 1 = next), computed from the UTC calendar.
 */
export function getUtcPeriodRange(
  timeframe: PeriodTimeframe,
  offset: number,
  now: Date = new Date()
): UtcPeriodRange {
  const year = now.getUTCFullYear();
  const month = now.getUTCMonth();

  let startMs: number;
  let nextStartMs: number;

  if (timeframe === 'monthly') {
    startMs = Date.UTC(year, month + offset, 1);
    nextStartMs = Date.UTC(year, month + offset + 1, 1);
  } else if (timeframe === 'quarterly') {
    const quarterStartMonth = Math.floor(month / 3) * 3 + offset * 3;
    startMs = Date.UTC(year, quarterStartMonth, 1);
    nextStartMs = Date.UTC(year, quarterStartMonth + 3, 1);
  } else {
    startMs = Date.UTC(year + offset, 0, 1);
    nextStartMs = Date.UTC(year + offset + 1, 0, 1);
  }

  return {
    start: startMs / SECOND,
    end: nextStartMs / SECOND - 1,
    startDate: new Date(startMs),
    endDate: new Date(nextStartMs - 86_400 * SECOND),
  };
}

/** UTC midnight (unix seconds) of an ISO `YYYY-MM-DD` date. */
export function utcDayStart(isoDate: string): number {
  const [y, m, d] = isoDate.split('-').map(Number);
  return Date.UTC(y, m - 1, d) / SECOND;
}

/** Last second (23:59:59 UTC) of an ISO `YYYY-MM-DD` date. */
export function utcDayEnd(isoDate: string): number {
  const [y, m, d] = isoDate.split('-').map(Number);
  return Date.UTC(y, m - 1, d + 1) / SECOND - 1;
}

/** `YYYY-MM-DD` of the UTC calendar day containing a unix-seconds timestamp. */
export function isoDateFromUtcTimestamp(ts: number): string {
  return new Date(ts * SECOND).toISOString().slice(0, 10);
}

/** `YYYY-MM-DD` of today's UTC calendar day. */
export function todayIsoUtc(now: Date = new Date()): string {
  return now.toISOString().slice(0, 10);
}
