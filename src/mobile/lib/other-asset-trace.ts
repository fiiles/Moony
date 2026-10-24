import type { AssetValuation, OtherAssetTransaction } from '@shared/schema';
import { utcDayFloor } from '@/utils/chart-axis';
import { amountOf } from '@/utils/positions';
import type { CostPoint } from './position-series';

/**
 * The line of an other asset (spec §8.8, ruling 6): its value in the price currency, `price ×
 * quantity held`, at every day the person put a price on it or traded it. The asset has no price
 * feed, so the line is a step line of the estimates; there is no history report to read.
 *
 * - Only the valuations in `priceCurrency` count: after the asset's currency was edited the older
 *   estimates are in another one, and native values are never compared across currencies (ADR 0012).
 * - A day takes the newest estimate on or before it times the quantity held after that day's
 *   trades. A trade day with no estimate behind it has no point (there is no price to value it at).
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

  // Oldest first; the sort is stable, so of two estimates on one day the later one stands
  const prices = valuations
    .filter((v) => v.currency === priceCurrency)
    .flatMap((v) => {
      const price = amountOf(v.value);
      return price === null ? [] : [{ day: utcDayFloor(v.valuedAt), price }];
    })
    .sort((a, b) => a.day - b.day);
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
 * one ("Vše"). Today's point, the last, always stays, so the line ends on the value the hero says.
 */
export function trimTrace(points: readonly CostPoint[], start: number | undefined): CostPoint[] {
  return points.filter(
    (point, i) => start === undefined || point.t >= start || i === points.length - 1
  );
}
