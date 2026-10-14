import { describe, expect, it } from 'vitest';
import type { OtherAsset } from '@shared/schema';
import { otherAssetRow, otherAssetsSummary } from './other-assets';

const DAY = 86_400;
/** Today: 1 EUR = 25 CZK; no rate for KWD. */
const convert = (amount: number, currency: string) =>
  currency === 'KWD' ? null : currency === 'EUR' ? amount * 25 : amount;
/** Before day 100: 1 EUR = 20 CZK, from day 100 on: 24 CZK; never a rate for KWD. */
const convertAt = (amount: number, currency: string, t: number) =>
  currency === 'KWD' ? null : currency === 'EUR' ? amount * (t < 100 * DAY ? 20 : 24) : amount;

/** The price is in the asset's currency unless `marketPriceCurrency` says otherwise. */
function asset(partial: Partial<OtherAsset>): OtherAsset {
  return {
    id: 'a',
    name: 'Gold',
    quantity: '3',
    marketPrice: '68500',
    currency: 'CZK',
    averagePurchasePrice: '52000',
    yieldType: 'none',
    yieldValue: null,
    createdAt: 0,
    updatedAt: 0,
    ...partial,
    marketPriceCurrency: partial.marketPriceCurrency ?? partial.currency ?? 'CZK',
  };
}

describe('otherAssetRow', () => {
  it('values quantity × price and the gain against the purchase average', () => {
    const row = otherAssetRow(asset({}), convert, convertAt);
    expect(row.value).toBe(205_500);
    expect(row.cost).toBe(156_000);
    expect(row.gain).toBe(49_500);
    expect(row.gainPct).toBeCloseTo(0.3173, 3);
    expect(row.yield).toBe(0);
  });

  it('computes the yearly yield by type', () => {
    const row = (partial: Partial<OtherAsset>) => otherAssetRow(asset(partial), convert, convertAt);
    expect(row({ yieldType: 'fixed', yieldValue: '2400' }).yield).toBe(2_400);
    expect(row({ yieldType: 'percent_purchase', yieldValue: '6' }).yield).toBeCloseTo(9_360);
    expect(row({ yieldType: 'percent_market', yieldValue: '1' }).yield).toBeCloseTo(2_055);
  });

  it('has no gain percentage without a purchase cost', () => {
    const row = otherAssetRow(asset({ averagePurchasePrice: '0' }), convert, convertAt);
    expect(row.cost).toBe(0);
    expect(row.gainPct).toBe(0);
    expect(row.gain).toBe(205_500);
  });

  it('converts the value today and every buy at its own day', () => {
    const eur = asset({
      currency: 'EUR',
      quantity: '2',
      marketPrice: '100',
      averagePurchasePrice: '90',
    });
    // Bought 1 at 80 EUR on day 10 (20 CZK) and 2 at 95 EUR on day 200 (24 CZK); 2 held
    const buys = [
      { quantity: 1, pricePerUnit: 80, currency: 'EUR', date: 10 * DAY },
      { quantity: 2, pricePerUnit: 95, currency: 'EUR', date: 200 * DAY },
    ];
    const row = otherAssetRow(eur, convert, convertAt, buys);
    expect(row.value).toBe(5_000);
    expect(row.cost).toBeCloseTo((2 * (80 * 20 + 2 * 95 * 24)) / 3);
    // Without transactions: the purchase average on the creation day
    const created = otherAssetRow({ ...eur, createdAt: 150 * DAY }, convert, convertAt);
    expect(created.cost).toBe(2 * 90 * 24);
  });

  it('values the price in its own currency and the purchase average in the asset currency', () => {
    // A CZK asset re-estimated in EUR: 2 held at 100 EUR, bought at 1 800 CZK each
    const reEstimated = (partial: Partial<OtherAsset>) =>
      otherAssetRow(
        asset({
          quantity: '2',
          marketPrice: '100',
          marketPriceCurrency: 'EUR',
          averagePurchasePrice: '1800',
          ...partial,
        }),
        convert,
        convertAt
      );
    const row = reEstimated({});
    expect(row.currency).toBe('CZK');
    expect(row.priceCurrency).toBe('EUR');
    expect(row.valueNative).toBe(200);
    expect(row.value).toBe(5_000);
    expect(row.cost).toBe(3_600);
    expect(row.gain).toBe(1_400);
    expect(row.gainNative).toBeNull();
    // A yield on the market value is in the price's currency, the others in the asset's
    const market = reEstimated({ yieldType: 'percent_market', yieldValue: '10' });
    expect(market.yieldCurrency).toBe('EUR');
    expect(market.yield).toBe(500);
    expect(reEstimated({ yieldType: 'percent_purchase', yieldValue: '10' }).yield).toBe(360);
    const fixed = reEstimated({ yieldType: 'fixed', yieldValue: '50' });
    expect(fixed.yieldCurrency).toBe('CZK');
    expect(fixed.yield).toBe(50);
  });

  it('has a native gain only when the price and the purchase average share a currency', () => {
    expect(otherAssetRow(asset({}), convert, convertAt).gainNative).toBe(49_500);
  });

  it('reports the price currency missing when only the price cannot convert', () => {
    const row = otherAssetRow(
      asset({
        quantity: '1',
        marketPrice: '700',
        marketPriceCurrency: 'KWD',
        averagePurchasePrice: '600',
        yieldType: 'percent_market',
        yieldValue: '1',
      }),
      convert,
      convertAt
    );
    expect(row.value).toBeNull();
    expect(row.cost).toBe(600);
    expect(row.gain).toBeNull();
    const s = otherAssetsSummary([row]);
    expect(s.missing).toEqual(['KWD']);
    expect(s.missingCost).toEqual([]);
  });

  it('keeps an unconvertible asset native and out of the totals', () => {
    const kwd = otherAssetRow(
      asset({
        id: 'k',
        currency: 'KWD',
        quantity: '1',
        marketPrice: '700',
        averagePurchasePrice: '600',
      }),
      convert,
      convertAt
    );
    expect(kwd.value).toBeNull();
    expect(kwd.cost).toBeNull();
    expect(kwd.gain).toBeNull();
    expect(kwd.gainPct).toBeNull();
    expect(kwd.valueNative).toBe(700);
    const gold = otherAssetRow(asset({}), convert, convertAt);
    const s = otherAssetsSummary([gold, kwd]);
    expect(s.value).toBe(205_500);
    expect(s.cost).toBe(156_000);
    expect(s.gain).toBe(49_500);
    expect(s.gainCost).toBe(156_000);
    expect(s.missing).toEqual(['KWD']);
    expect(s.missingCost).toEqual(['KWD']);
  });
});
