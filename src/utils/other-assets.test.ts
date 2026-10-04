import { describe, expect, it } from 'vitest';
import type { OtherAsset } from '@shared/schema';
import { otherAssetRow } from './other-assets';

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
  };
}

describe('otherAssetRow', () => {
  it('values quantity × price and the gain against the purchase average', () => {
    const row = otherAssetRow(asset({}));
    expect(row.valueCzk).toBe(205_500);
    expect(row.costCzk).toBe(156_000);
    expect(row.gainCzk).toBe(49_500);
    expect(row.gainPct).toBeCloseTo(0.3173, 3);
    expect(row.yieldCzk).toBe(0);
  });

  it('computes the yearly yield by type', () => {
    expect(otherAssetRow(asset({ yieldType: 'fixed', yieldValue: '2400' })).yieldCzk).toBe(2_400);
    expect(
      otherAssetRow(asset({ yieldType: 'percent_purchase', yieldValue: '6' })).yieldCzk
    ).toBeCloseTo(9_360);
    expect(
      otherAssetRow(asset({ yieldType: 'percent_market', yieldValue: '1' })).yieldCzk
    ).toBeCloseTo(2_055);
  });

  it('has no gain percentage without a purchase cost', () => {
    const row = otherAssetRow(asset({ averagePurchasePrice: '0' }));
    expect(row.costCzk).toBe(0);
    expect(row.gainPct).toBe(0);
    expect(row.gainCzk).toBe(205_500);
  });
});
