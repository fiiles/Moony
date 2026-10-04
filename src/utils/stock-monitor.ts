/**
 * Pure helpers for the Stock Monitor (watchlist) domain.
 * All prices arrive as money-as-TEXT strings in the stock's native currency.
 */

import { currencyDecimals } from '@shared/currencies';
import { getFormatters } from '@/lib/format';

export const CHART_PERIODS = ['1D', '5D', '1M', '6M', 'YTD', '1Y', '5Y', 'MAX'] as const;
export type ChartPeriod = (typeof CHART_PERIODS)[number];

export interface DayChange {
  abs: number;
  pct: number;
}

/** Day change vs yesterday's close (spec D4). Null when inputs are unusable. */
export function dayChange(
  currentPrice: string | null,
  previousClose: string | null
): DayChange | null {
  const current = parseFloat(currentPrice ?? '');
  const previous = parseFloat(previousClose ?? '');
  if (!isFinite(current) || !isFinite(previous) || previous <= 0) return null;
  return { abs: current - previous, pct: ((current - previous) / previous) * 100 };
}

/**
 * Change of the current price against an arbitrary reference price (e.g. the
 * first data point of the selected chart horizon). Null when unusable.
 */
export function changeFromReference(current: number, reference: number): DayChange | null {
  if (!isFinite(current) || !isFinite(reference) || reference <= 0) return null;
  return { abs: current - reference, pct: ((current - reference) / reference) * 100 };
}

/**
 * Informative distance from the current price to the user's target, in
 * percent (positive = the price would have to rise). No reached/crossed
 * semantics — the target is a personal reference point, nothing more.
 */
export function targetDistance(
  currentPrice: string | null,
  targetPrice: string | null
): number | null {
  const current = parseFloat(currentPrice ?? '');
  const target = parseFloat(targetPrice ?? '');
  if (!isFinite(current) || !isFinite(target) || current <= 0 || target <= 0) return null;
  return ((target - current) / current) * 100;
}

/**
 * Render with as many decimals as fit: starts at `decimals` and steps down one
 * at a time while the result is longer than `maxLength`. Without a maxLength
 * the preferred precision is kept as-is; when even 0 decimals is too long the
 * caller is expected to let the value wrap.
 */
function fitDecimals(
  build: (decimals: number) => string,
  decimals: number,
  maxLength?: number
): string {
  let result = build(decimals);
  if (maxLength == null) return result;
  for (let d = decimals - 1; d >= 0 && result.length > maxLength; d--) {
    result = build(d);
  }
  return result;
}

/**
 * Format a price in the stock's native currency (spec D5).
 * `maxLength` fits the result into a narrow container (e.g. a stat card) by
 * dropping the decimals rather than letting the value overflow; `signed` adds
 * an explicit "+" (change columns).
 */
export function formatNativePrice(
  value: number,
  currency: string | null,
  locale: string,
  maxLength?: number,
  signed = false
): string {
  if (!isFinite(value)) return '—';
  const fmt = getFormatters(locale);
  const build = (decimals: number) => fmt.money(value, currency ?? '', { decimals, signed });
  return fitDecimals(build, currencyDecimals(currency), maxLength);
}

/**
 * Newest price-fetch timestamp (unix seconds) across watchlist rows, or null
 * when nothing has been fetched yet — drives the "prices updated at" footnote.
 */
export function latestFetchedAt(rows: Array<{ priceFetchedAt: number | null }>): number | null {
  const stamps = rows.map((r) => r.priceFetchedAt).filter((t): t is number => t != null);
  return stamps.length > 0 ? Math.max(...stamps) : null;
}

/** trailing_dividend_yield is stored as a raw fraction string ("0.0044" = 0.44 %) */
export function percentFromFraction(fraction: string | null): number | null {
  const f = parseFloat(fraction ?? '');
  return isFinite(f) ? f * 100 : null;
}

/**
 * Compact market cap ("$3.12T"); null when absent.
 * `maxLength` fits the result into a narrow container by dropping the
 * decimals ("327,06 mld. US$" -> "327 mld. US$") rather than overflowing.
 */
export function formatMarketCap(
  marketCap: string | null,
  currency: string | null,
  locale: string,
  maxLength?: number
): string | null {
  const v = parseFloat(marketCap ?? '');
  if (!isFinite(v)) return null;
  const fmt = getFormatters(locale);
  const build = (decimals: number) => fmt.money(v, currency ?? '', { compact: true, decimals });
  return fitDecimals(build, 2, maxLength);
}
