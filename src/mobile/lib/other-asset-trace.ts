import type { AssetValuation, OtherAssetTransaction } from '@shared/schema';
import { utcDayFloor } from '@/utils/chart-axis';
import { amountOf } from '@/utils/positions';
import type { CostPoint } from './position-series';

/**
 * The line of an other asset (spec §8.8, ruling 6): its value in the price currency, `price ×
 * quantity held`, at every day the person put a price on it or traded it. The asset has no price
 * feed, so the line is a step line of the estimates; there is no history report to read.
 *
 * - Only prices in `priceCurrency` count: after the asset's currency was edited the older
 *   estimates are in another one, and native values are never compared across currencies (ADR 0012).
 * - A day takes the newest estimate on or before it times the quantity held after that day's trades.
 * - A trade is a price observation too (ruling P4) — its own `pricePerUnit`, when it is in the
 *   price currency (any case) — but only until a valuation in that currency is on or before its day:
 *   a fresh asset with a first purchase and no valuation starts at that purchase, and a valuation
 *   always wins over the trade price. A trade in another currency, or one with no estimate of
 *   any kind behind it, has no point (there is no price to value it at).
 * - The quantity comes from the trades; an asset with no trades at all (entered before they
 *   existed) is held at `currentQuantity` on every day (ruling P3), the only quantity known.
 * - Today's value, `currentPrice × currentQuantity`, is the last point: the day's own when a point
 *   already sits on today's UTC day, a new one otherwise. Days after today are left out.
 */
export function otherAssetTrace(
  valuations: readonly AssetValuation[],
  transactions: readonly OtherAssetTransaction[],
  priceCurrency: string,
  currentPrice: number,
  currentQuantity: number,
  nowSec: number
): CostPoint[] {
  const today = utcDayFloor(nowSec);
  const inPriceCurrency = (code: string) => code.toUpperCase() === priceCurrency.toUpperCase();

  const valued = valuations
    .filter((v) => inPriceCurrency(v.currency))
    .flatMap((v) => {
      const price = amountOf(v.value);
      return price === null ? [] : [{ day: utcDayFloor(v.valuedAt), price }];
    });
  // A trade the valuations do not reach back to: its own price is the only estimate of its day
  const traded = transactions.flatMap((tx) => {
    const day = utcDayFloor(tx.transactionDate);
    const price = amountOf(tx.pricePerUnit);
    return price === null || !inPriceCurrency(tx.currency) || valued.some((v) => v.day <= day)
      ? []
      : [{ day, price }];
  });
  // Oldest first; the sort is stable, so of two estimates on one day the later one stands
  const prices = [...valued, ...traded].sort((a, b) => a.day - b.day);
  const trades = transactions.map((tx) => ({
    day: utcDayFloor(tx.transactionDate),
    delta: (tx.type === 'sell' ? -1 : 1) * (amountOf(tx.quantity) ?? 0),
  }));

  const priceOn = (day: number): number | undefined =>
    prices.reduce<number | undefined>((found, p) => (p.day <= day ? p.price : found), undefined);
  const heldOn = (day: number): number =>
    trades.length === 0
      ? currentQuantity
      : Math.max(
          0,
          trades.reduce((sum, trade) => (trade.day <= day ? sum + trade.delta : sum), 0)
        );

  const days = [...new Set([...prices.map((p) => p.day), ...trades.map((tx) => tx.day)])]
    .filter((day) => day <= today)
    .sort((a, b) => a - b);
  const points = days.flatMap((day) => {
    const price = priceOn(day);
    return price === undefined ? [] : [{ t: day, value: price * heldOn(day) }];
  });

  const value = currentPrice * currentQuantity;
  const last = points[points.length - 1];
  if (last !== undefined && last.t === today) return [...points.slice(0, -1), { ...last, value }];
  return [...points, { t: Math.floor(nowSec), value }];
}

/**
 * The part of a trace a period shows: the points from the period's `start` on, all of them without
 * one ("Vše"). A step line holds its value, so the estimate in force at `start` — the newest point
 * before it — is carried onto `start` (unless a point sits there already): an asset not revalued
 * inside the period is a flat line across it, not a blank. Today's point, the last, always stays,
 * so the line ends on the value the hero says; with nothing earlier it is the only point.
 */
export function trimTrace(points: readonly CostPoint[], start: number | undefined): CostPoint[] {
  if (start === undefined || points.length === 0) return [...points];
  const first = points.findIndex((point) => point.t >= start);
  // Nothing at or after the start (never for today's point): it alone stays
  const from = first === -1 ? points.length - 1 : first;
  const inside = points.slice(from);
  const before = points[from - 1];
  if (before === undefined || inside[0].t <= start) return inside;
  return [{ ...before, t: start }, ...inside];
}
