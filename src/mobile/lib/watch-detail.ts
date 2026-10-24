import { priceDecimals } from '@shared/currencies';
import type { StockPricePoint, TargetDirection } from '@shared/schema';
import type { Formatters } from '@/lib/format';
import type { ChartPeriod } from '@/utils/period';
import {
  inferTargetDirection,
  targetReached,
  type ChartPeriod as WatchPeriod,
} from '@/utils/stock-monitor';
import { parseAmountInput } from './amount-input';
import type { CostPoint } from './position-series';

export type { WatchPeriod };

const DAY = 86_400;

/** A price string of the wire as a number; `null` when it is missing or not one. */
export function priceOf(value: string | null): number | null {
  const n = parseFloat(value ?? '');
  return Number.isFinite(n) ? n : null;
}

/**
 * A watched stock's price as text, in its own currency and with the decimals of a price (none from
 * 1000 up); `signed` writes the "+" of a change. Never converted and never masked: the price is
 * Yahoo's public number, not an amount of the person's (spec §8.8; the watch files are the one
 * place `fmt.money` is called directly).
 */
export function watchPriceText(
  fmt: Pick<Formatters, 'money'>,
  value: number,
  currency: string | null,
  signed = false
): string {
  return fmt.money(value, currency ?? '', { decimals: priceDecimals(value, currency), signed });
}

/**
 * The Yahoo range that draws a phone period (ruling 7), and where to cut it when the range is
 * longer than the period: 90 days come from the six-month range and are trimmed to the last 90
 * days (`watchPoints`). The intraday ranges (1D, 5D) stay on the desktop.
 */
export function watchRange(
  period: ChartPeriod,
  nowSec: number
): { range: WatchPeriod; from: number | null } {
  switch (period) {
    case '30D':
      return { range: '1M', from: null };
    case '90D':
      return { range: '6M', from: nowSec - 90 * DAY };
    case 'YTD':
      return { range: 'YTD', from: null };
    case '1Y':
      return { range: '1Y', from: null };
    case '5Y':
      return { range: '5Y', from: null };
    case 'All':
      return { range: 'MAX', from: null };
  }
}

/**
 * Where a watched stock stands against its target: `none` without one, `reached` once the price
 * has crossed it in its direction, `pending` otherwise — also while there is no price to compare.
 */
export function watchTargetState(
  current: number | null,
  target: number | null,
  direction: TargetDirection | null
): 'none' | 'reached' | 'pending' {
  if (target === null || !Number.isFinite(target)) return 'none';
  return targetReached(current, target, direction) ? 'reached' : 'pending';
}

/**
 * The line of the watch detail (the desktop detail's `points`): the range as prices, trimmed to
 * `from` when the range is longer than the period, and today's price as its last point — a new
 * one when it was fetched after the range ends, the last point's own otherwise (an intraday
 * price replaces the day's close). With a target every point carries it as `cost`, which the
 * hero draws as its dashed line; without one no point does. A price without any point to meet
 * (nothing in the range) draws nothing. `nowSec` stamps today when the fetch time is unknown.
 */
export function watchPoints(
  range: readonly StockPricePoint[],
  from: number | null,
  current: number | null,
  fetchedAt: number | null,
  target: number | null,
  nowSec = Math.floor(Date.now() / 1000)
): CostPoint[] {
  const points: CostPoint[] = range
    .filter((point) => from === null || point.timestamp >= from)
    .map((point) => ({ t: point.timestamp, value: point.price }));

  if (current !== null && Number.isFinite(current) && points.length > 0) {
    const last = points[points.length - 1];
    const at = fetchedAt ?? nowSec;
    if (at > last.t) points.push({ t: at, value: current });
    else points[points.length - 1] = { ...last, value: current };
  }

  return target !== null && Number.isFinite(target)
    ? points.map((point) => ({ ...point, cost: target }))
    : points;
}

/**
 * What a new target waits for (the desktop target dialog's rule): what the person picked; else,
 * with nothing typed, the stored direction (a rise when there is none); else the stored direction
 * of an unchanged price; else the one the typed price implies against today's (a price under it
 * waits for a dip).
 */
export function watchTargetDirection({
  chosen,
  draft,
  current,
  stored,
}: {
  chosen: TargetDirection | null;
  /** The typed price; `null` while it is empty or not a number. */
  draft: number | null;
  current: number | null;
  /** The target the stock has now. */
  stored: { price: number; direction: TargetDirection } | null;
}): TargetDirection {
  if (chosen !== null) return chosen;
  if (draft === null) return stored?.direction ?? 'above';
  if (stored !== null && draft === stored.price) return stored.direction;
  return inferTargetDirection(draft, current);
}

/** What a typed target price comes to: the number and the decimal text the backend stores, or what is wrong with it. */
export interface WatchTargetForm {
  price: number | null;
  text: string | null;
  error?: 'required' | 'invalid' | 'positive';
}

/**
 * Reads the target field as the phone's keypad types it (`parseAmountInput`: a comma or a point,
 * spaces as thousands): a positive number is a target; an empty field, text that is not a number
 * and zero or less are not. Removing a target is its own button, so an empty field is not "none".
 */
export function watchTargetForm(draft: string): WatchTargetForm {
  if (draft.trim() === '') return { price: null, text: null, error: 'required' };
  const text = parseAmountInput(draft);
  const price = text === null ? NaN : Number(text);
  if (!Number.isFinite(price)) return { price: null, text: null, error: 'invalid' };
  if (price <= 0) return { price: null, text: null, error: 'positive' };
  return { price, text };
}

/** The shortest Yahoo range that surely holds the day the stock was followed (the desktop's `periodCovering`). */
export function watchSincePeriod(followedAt: number, nowSec: number): WatchPeriod {
  const age = (nowSec - followedAt) / DAY;
  if (age <= 28) return '1M';
  if (age <= 180) return '6M';
  if (age <= 365) return '1Y';
  if (age <= 5 * 365) return '5Y';
  return 'MAX';
}

/** The price of the point nearest `at` (the first of two equally near); `null` for a series without points. */
export function watchPriceAt(series: readonly StockPricePoint[], at: number): number | null {
  if (series.length === 0) return null;
  let best = series[0];
  for (const point of series) {
    if (Math.abs(point.timestamp - at) < Math.abs(best.timestamp - at)) best = point;
  }
  return best.price;
}
