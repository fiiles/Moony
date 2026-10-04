import { describe, it, expect } from 'vitest';
import type { RealEstate } from '@shared/schema';
import { realEstateUpdatePayload } from './real-estate-payload';

const property: RealEstate = {
  id: 're-1',
  name: 'Flat',
  address: 'Main St 1',
  type: 'rental',
  purchasePrice: '5000000',
  purchasePriceCurrency: 'CZK',
  marketPrice: '6500000',
  marketPriceCurrency: 'CZK',
  monthlyRent: '20000',
  monthlyRentCurrency: 'CZK',
  recurringCosts: [{ name: 'Fund', amount: 1500, frequency: 'monthly', currency: 'CZK' }],
  photos: ['a.jpg'],
  notes: 'old note',
  createdAt: 1,
  updatedAt: 2,
} as RealEstate;

describe('realEstateUpdatePayload', () => {
  it('sends every field back unchanged (the backend update replaces the whole property)', () => {
    expect(realEstateUpdatePayload(property)).toEqual({
      name: 'Flat',
      address: 'Main St 1',
      type: 'rental',
      purchasePrice: '5000000',
      purchasePriceCurrency: 'CZK',
      marketPrice: '6500000',
      marketPriceCurrency: 'CZK',
      monthlyRent: '20000',
      monthlyRentCurrency: 'CZK',
      recurringCosts: property.recurringCosts,
      photos: ['a.jpg'],
      notes: 'old note',
    });
  });

  it('applies only the overridden fields', () => {
    const payload = realEstateUpdatePayload(property, { notes: 'new note' });
    expect(payload.notes).toBe('new note');
    expect(payload.monthlyRent).toBe('20000');
    expect(payload.recurringCosts).toBe(property.recurringCosts);
  });

  it('turns empty notes into "no notes" instead of storing an empty string', () => {
    expect(realEstateUpdatePayload(property, { notes: '   ' }).notes).toBeUndefined();
    expect(realEstateUpdatePayload({ ...property, notes: null }).notes).toBeUndefined();
  });

  it('keeps a missing rent currency undefined', () => {
    expect(
      realEstateUpdatePayload({ ...property, monthlyRent: null, monthlyRentCurrency: null })
        .monthlyRentCurrency
    ).toBeUndefined();
  });
});
