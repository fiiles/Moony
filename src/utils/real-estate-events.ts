/**
 * Purchase and revaluation marks for the aggregate trend of the real estate list (design
 * system §9: an aggregate line carries only the events that explain its jumps, like the buy
 * and sell days of the stocks list). Pure: no DOM, no locale, no hooks. The page converts the
 * amounts through the `convert` it passes in and formats the tooltips.
 */

import { recordCurrency } from '@shared/currencies';
import { utcDayFloor } from './chart-axis';
import type { ChartEvent } from './chart-scale';
import { dailyValuations } from './valuation-trace';

/** What a property has to offer for its purchase mark. */
export interface PropertyLike {
  id: string;
  name: string;
  purchasePrice: string;
  /** Currency of `purchasePrice`; CZK when missing. */
  purchasePriceCurrency?: string | null;
  /** UTC day the property was bought; `null` while unknown. */
  purchaseDate: number | null;
  /** Unix seconds; the purchase day while that is unknown. */
  createdAt: number;
}

/** What a stored estimate has to offer for its mark. */
export interface ValuationLike {
  id: string;
  /** The property the estimate belongs to. */
  assetId: string;
  value: string;
  /** Currency of `value`; CZK when missing. */
  currency?: string | null;
  /** UTC day the estimate applies to. */
  valuedAt: number;
  /** When the row was entered; orders estimates of one day. */
  createdAt: number;
}

/** The purchase, the first estimate of a property, or a later estimate. */
export type RealEstateEventKind = 'purchase' | 'first' | 'revaluation';

/** One mark on the list trend: a purchase (`buy`) or an estimate (`mark`). */
export interface RealEstateEvent extends ChartEvent {
  type: 'buy' | 'mark';
  kind: RealEstateEventKind;
  propertyId: string;
  name: string;
  /** The price or estimate as stored: what a tooltip shows while its day has no rate. */
  native: { amount: number; currency: string };
  /** The same in the report currency at the rates of its own day; `null` without a rate. */
  value: number | null;
  /**
   * An estimate against the previous one of its property, as a ratio (0.05 = +5 %); `null` for
   * a purchase, a first estimate and when the two cannot be compared.
   */
  change: number | null;
}

/** Into the report currency at the rates of the amount's own day (`null` without a rate). */
export type EventConvert = (amount: number, currency: string, date: number) => number | null;

const amountOf = (value: string | null | undefined): number => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
};

/** An amount in the report currency; zero needs no rate. */
const toReport = (convert: EventConvert, amount: number, currency: string, date: number) =>
  amount === 0 ? 0 : convert(amount, currency, date);

/**
 * Ratio of one estimate to the one before it. Estimates in one currency compare as stored, so
 * an exchange rate never shows up as a revaluation; across currencies the converted values
 * compare. `null` when the earlier estimate is zero or a side has no rate.
 */
function changeBetween(
  previous: { amount: number; currency: string; value: number | null },
  current: { amount: number; currency: string; value: number | null }
): number | null {
  if (previous.currency === current.currency) {
    return previous.amount > 0 ? current.amount / previous.amount - 1 : null;
  }
  if (previous.value === null || current.value === null) return null;
  return previous.value > 0 ? current.value / previous.value - 1 : null;
}

/**
 * The marks of the list trend, sorted by day with the purchase before an estimate.
 *
 * - A property is marked at its purchase when the day and a price are known (a price of 0 is the
 *   form's "not entered"), like the detail's value trace.
 * - Every estimate is marked, one per day (the newest of that day). Only the first estimate of a
 *   property is dropped, and only when it is the initial one: dated on the purchase day, for the
 *   purchase price and in its currency, so it would sit under the purchase mark.
 * - Estimates of properties that are not in `properties` (a deleted one) are ignored.
 * - Every amount converts on its own day through `convert`; a mark without a rate stays, with
 *   its native amount, so the event is never lost for want of a rate.
 */
export function realEstateEvents(
  properties: readonly PropertyLike[],
  valuations: readonly ValuationLike[],
  convert: EventConvert
): RealEstateEvent[] {
  const events: RealEstateEvent[] = [];
  const purchases = new Map<string, { day: number; amount: number; currency: string }>();

  for (const property of properties) {
    if (property.purchaseDate === null) continue;
    const amount = amountOf(property.purchasePrice);
    if (!(amount > 0)) continue;
    const day = utcDayFloor(property.purchaseDate);
    const currency = recordCurrency(property.purchasePriceCurrency);
    purchases.set(property.id, { day, amount, currency });
    events.push({
      id: `purchase-${property.id}`,
      t: day,
      type: 'buy',
      kind: 'purchase',
      propertyId: property.id,
      name: property.name,
      native: { amount, currency },
      value: toReport(convert, amount, currency, day),
      change: null,
    });
  }

  const names = new Map(properties.map((p) => [p.id, p.name]));
  const byProperty = new Map<string, ValuationLike[]>();
  for (const valuation of valuations) {
    if (!names.has(valuation.assetId)) continue;
    const list = byProperty.get(valuation.assetId);
    if (list) list.push(valuation);
    else byProperty.set(valuation.assetId, [valuation]);
  }

  for (const [propertyId, rows] of byProperty) {
    const name = names.get(propertyId) ?? '';
    const purchase = purchases.get(propertyId);
    const days = dailyValuations(
      rows.map((row) => ({
        row,
        t: utcDayFloor(row.valuedAt),
        createdAt: row.createdAt,
      }))
    );
    let previous: { amount: number; currency: string; value: number | null } | null = null;
    for (const [index, { row, t }] of days.entries()) {
      const amount = amountOf(row.value);
      const currency = recordCurrency(row.currency);
      const value = toReport(convert, amount, currency, t);
      const current = { amount, currency, value };
      // The initial estimate of a property bought and valued on one day for one price
      const initial =
        index === 0 &&
        purchase !== undefined &&
        purchase.day === t &&
        purchase.currency === currency &&
        purchase.amount === amount;
      if (!initial) {
        events.push({
          id: `valuation-${row.id}`,
          t,
          type: 'mark',
          kind: index === 0 ? 'first' : 'revaluation',
          propertyId,
          name,
          native: { amount, currency },
          value,
          change: previous === null ? null : changeBetween(previous, current),
        });
      }
      previous = current;
    }
  }

  return events.sort(
    (a, b) =>
      a.t - b.t ||
      (a.type === b.type ? 0 : a.type === 'buy' ? -1 : 1) ||
      (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)
  );
}

/**
 * UTC day of the earliest purchase or estimate (the creation day of a property whose purchase
 * day is unknown): where the dated exchange rates have to start so every mark can convert.
 */
export function firstRealEstateDay(
  properties: readonly Pick<PropertyLike, 'purchaseDate' | 'createdAt'>[],
  valuations: readonly Pick<ValuationLike, 'valuedAt'>[]
): number | undefined {
  let earliest: number | undefined;
  for (const property of properties) {
    const day = property.purchaseDate ?? property.createdAt;
    if (earliest === undefined || day < earliest) earliest = day;
  }
  for (const valuation of valuations) {
    if (earliest === undefined || valuation.valuedAt < earliest) earliest = valuation.valuedAt;
  }
  return earliest;
}
