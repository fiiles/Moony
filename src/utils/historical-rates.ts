/**
 * Pure helpers for converting stored portfolio history rows into the display
 * currency using historical (per-day) exchange rates.
 *
 * Rates come from the `get_exchange_rates_for_date_range` command: a map of
 * midnight-UTC unix timestamp -> { currency -> CZK per unit }. Each day map
 * contains CZK: 1 plus whichever currencies have an `exchange_rate_history`
 * row that day. Days without any snapshot (weekends/holidays) have no entry.
 *
 * Fallback chain per data point (spec 2026-08-06-calculation-fixes §3):
 *   1. Native breakdown converted with that day's rates (closest earlier day),
 *      but only when the breakdown is consistent with the stored CZK total
 *      (snapshot backfill used to write partial breakdowns; those rows render
 *      via the CZK total instead of the fragment).
 *   2. A bucket currency missing from the day map -> that bucket converts at
 *      today's rates (the consistency check is skipped, it cannot be computed
 *      like-for-like).
 *   3. No usable day rates (or no breakdown, or an inconsistent breakdown) ->
 *      stored CZK total converted at today's rates (graceful degradation).
 *   CZK display always returns the stored CZK total untouched.
 *
 * Framework-free on purpose so it stays unit-testable (see testing policy).
 */

export type DayRates = Record<string, number>;
export type RatesByDay = Record<number, DayRates>;

/** Converts `amount` in `fromCurrency` to the display currency at today's rates. */
export type ConvertTodayFn = (amount: number, fromCurrency: string) => number;

export const SECONDS_PER_DAY = 86400;

/** Floor a unix timestamp to its midnight-UTC day key. */
export function dayKeyFor(ts: number): number {
  return Math.floor(ts / SECONDS_PER_DAY) * SECONDS_PER_DAY;
}

// Sorted day keys are cached per rates object so per-point lookups stay cheap
// even for multi-year charts.
const sortedKeysCache = new WeakMap<RatesByDay, number[]>();

function sortedDayKeys(ratesByDay: RatesByDay): number[] {
  let keys = sortedKeysCache.get(ratesByDay);
  if (!keys) {
    keys = Object.keys(ratesByDay)
      .map(Number)
      .sort((a, b) => a - b);
    sortedKeysCache.set(ratesByDay, keys);
  }
  return keys;
}

/**
 * Rates for `dayTs` (a midnight-UTC day key): the exact day if present,
 * otherwise the closest earlier day, otherwise undefined.
 */
export function resolveRatesForDay(
  ratesByDay: RatesByDay | undefined,
  dayTs: number
): DayRates | undefined {
  if (!ratesByDay) return undefined;
  const exact = ratesByDay[dayTs];
  if (exact) return exact;

  const keys = sortedDayKeys(ratesByDay);
  // Binary search for the greatest key strictly below dayTs.
  let lo = 0;
  let hi = keys.length - 1;
  let best = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (keys[mid] < dayTs) {
      best = keys[mid];
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return best >= 0 ? ratesByDay[best] : undefined;
}

function parseBreakdown(breakdownJson: string | null | undefined): Record<string, number> {
  if (!breakdownJson) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(breakdownJson);
  } catch {
    return {};
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return {};

  const breakdown: Record<string, number> = {};
  for (const [currency, amount] of Object.entries(parsed)) {
    if (typeof amount === 'number' && Number.isFinite(amount)) {
      breakdown[currency] = amount;
    }
  }
  return breakdown;
}

function isUsableRate(rate: number | undefined): rate is number {
  return rate !== undefined && Number.isFinite(rate) && rate > 0;
}

/**
 * Maximum relative difference tolerated between a breakdown's CZK equivalent
 * (at that day's rates) and the stored CZK total (at snapshot-time rates).
 * Rate drift between the two sources is a few percent at most; a larger gap
 * means the breakdown does not cover the whole portfolio.
 */
export const BREAKDOWN_CONSISTENCY_TOLERANCE = 0.25;

/**
 * Convert one history data point (native breakdown + stored CZK total) into
 * the display currency using that day's rates, per the fallback chain above.
 */
export function convertBreakdownAtDay(
  breakdownJson: string | null | undefined,
  czkTotal: number,
  dayRates: DayRates | undefined,
  displayCurrency: string,
  convertToday: ConvertTodayFn
): number {
  if (displayCurrency === 'CZK') return czkTotal;

  const breakdown = parseBreakdown(breakdownJson);
  const displayRate = dayRates?.[displayCurrency];
  if (Object.keys(breakdown).length === 0 || !dayRates || !isUsableRate(displayRate)) {
    // No breakdown or no usable day rates: stored CZK total at today's rates.
    return convertToday(czkTotal, 'CZK');
  }

  // Consistency guard: when every bucket has a usable day rate, the breakdown's
  // CZK equivalent must roughly match the stored CZK total. A large gap means a
  // partial breakdown (e.g. written by the snapshot backfill from incomplete
  // per-ticker history) — render via the CZK total instead.
  let czkEquivalent = 0;
  let allBucketsHaveDayRates = true;
  for (const [currency, amount] of Object.entries(breakdown)) {
    const rate = dayRates[currency];
    if (isUsableRate(rate)) {
      czkEquivalent += amount * rate;
    } else {
      allBucketsHaveDayRates = false;
    }
  }
  if (
    allBucketsHaveDayRates &&
    czkTotal > 0 &&
    Math.abs(czkEquivalent - czkTotal) / czkTotal > BREAKDOWN_CONSISTENCY_TOLERANCE
  ) {
    return convertToday(czkTotal, 'CZK');
  }

  return Object.entries(breakdown).reduce((sum, [currency, amount]) => {
    const rate = dayRates[currency];
    if (isUsableRate(rate)) {
      // native -> CZK at that day's rate -> display at that day's rate
      return sum + (amount * rate) / displayRate;
    }
    // Currency missing from that day's map: convert this bucket at today's
    // rates (we have no per-bucket CZK equivalent to use instead).
    return sum + convertToday(amount, currency);
  }, 0);
}

/**
 * Convert a stored CZK amount into the display currency at that day's rate,
 * falling back to today's rates when the day rate is unavailable.
 */
/**
 * Build a `DatedConvertFn` (shared/calculations/cost-basis.ts) over a
 * prefetched rates range: converts between any two currencies at the rates
 * of the transaction's day (closest earlier day when the exact day has no
 * snapshot), falling back to `convertToday` when either rate is missing.
 */
export function makeDatedConvertFn(
  ratesByDay: RatesByDay | undefined,
  convertToday: (amount: number, from: string, to: string) => number
): (amount: number, from: string, to: string, dateTs: number) => number {
  return (amount, from, to, dateTs) => {
    if (from === to) return amount;
    const dayRates = resolveRatesForDay(ratesByDay, dayKeyFor(dateTs));
    // Rates are CZK-per-unit; CZK itself is 1.
    const fromRate = from === 'CZK' ? 1 : dayRates?.[from];
    const toRate = to === 'CZK' ? 1 : dayRates?.[to];
    if (isUsableRate(fromRate) && isUsableRate(toRate)) {
      return (amount * fromRate) / toRate;
    }
    return convertToday(amount, from, to);
  };
}

export function convertCzkAtDay(
  czkAmount: number,
  dayRates: DayRates | undefined,
  displayCurrency: string,
  convertToday: ConvertTodayFn
): number {
  if (displayCurrency === 'CZK') return czkAmount;
  const displayRate = dayRates?.[displayCurrency];
  if (isUsableRate(displayRate)) return czkAmount / displayRate;
  return convertToday(czkAmount, 'CZK');
}
