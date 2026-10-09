import { describe, expect, it } from 'vitest';
import type { Loan, RealEstate } from '@shared/schema';
import { propertiesSummary, propertyRow } from './real-estate';

const DAY = 86_400;
/** Today: 1 EUR = 25 CZK; no rate for KWD. */
const convert = (amount: number, currency: string) =>
  currency === 'KWD' ? null : currency === 'EUR' ? amount * 25 : amount;
/** Before day 100: 1 EUR = 20 CZK, later 24; never a rate for KWD. */
const convertAt = (amount: number, currency: string, t: number) =>
  currency === 'KWD' ? null : currency === 'EUR' ? amount * (t < 100 * DAY ? 20 : 24) : amount;

function property(partial: Partial<RealEstate>): RealEstate {
  return {
    id: 'p',
    name: 'Byt',
    address: '',
    type: 'investment',
    purchasePrice: '4000000',
    purchasePriceCurrency: 'CZK',
    purchaseDate: 10 * DAY,
    marketPrice: '5000000',
    marketPriceCurrency: 'CZK',
    monthlyRent: '20000',
    monthlyRentCurrency: 'CZK',
    recurringCosts: [],
    photos: [],
    notes: null,
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}

const loan = (outstandingBalance: string, currency = 'CZK') =>
  ({ id: 'l', outstandingBalance, currency }) as Loan;

describe('propertyRow', () => {
  it('derives appreciation, yield, equity and LTV', () => {
    const r = propertyRow(property({}), [loan('2000000')], false, convert, convertAt);
    expect(r.value).toBe(5_000_000);
    expect(r.appreciation).toBe(1_000_000);
    expect(r.rentYear).toBe(240_000);
    expect(r.grossYield).toBeCloseTo(0.048);
    expect(r.equity).toBe(3_000_000);
    expect(r.ltv).toBeCloseTo(0.4);
  });

  it('converts the value today and the purchase at its own day', () => {
    const r = propertyRow(
      property({
        marketPrice: '200000',
        marketPriceCurrency: 'EUR',
        purchasePrice: '150000',
        purchasePriceCurrency: 'EUR',
        purchaseDate: 10 * DAY,
      }),
      [],
      false,
      convert,
      convertAt
    );
    expect(r.value).toBe(5_000_000);
    expect(r.purchase).toBe(3_000_000);
    expect(r.appreciation).toBe(2_000_000);
    // An unknown purchase day falls back to the creation day
    const undated = propertyRow(
      property({
        purchasePrice: '100',
        purchasePriceCurrency: 'EUR',
        purchaseDate: null,
        createdAt: 200 * DAY,
      }),
      [],
      false,
      convert,
      convertAt
    );
    expect(undated.purchase).toBe(2_400);
  });

  it('keeps an unconvertible property native and out of the totals', () => {
    const czk = propertyRow(property({ id: 'a' }), [], false, convert, convertAt);
    const kwd = propertyRow(
      property({
        id: 'b',
        marketPrice: '90000',
        marketPriceCurrency: 'KWD',
        purchasePrice: '80000',
        purchasePriceCurrency: 'KWD',
        monthlyRent: '0',
      }),
      [loan('1000000')],
      false,
      convert,
      convertAt
    );
    expect(kwd.value).toBeNull();
    expect(kwd.purchase).toBeNull();
    expect(kwd.equity).toBeNull();
    expect(kwd.loans).toBe(1_000_000);
    expect(kwd.valueNative).toEqual({ amount: 90_000, currency: 'KWD' });
    const s = propertiesSummary([czk, kwd]);
    expect(s.value).toBe(5_000_000);
    expect(s.appreciation).toBe(1_000_000);
    expect(s.equity).toBe(5_000_000);
    expect(s.loans).toBe(0);
    expect(s.missing).toEqual(['KWD']);
    expect(s.missingPurchase).toEqual(['KWD']);
  });

  it('leaves an excluded personal residence out of the totals but in its own value', () => {
    const home = propertyRow(property({ type: 'personal' }), [], true, convert, convertAt);
    expect(home.excluded).toBe(true);
    const s = propertiesSummary([home]);
    expect(s.value).toBe(0);
    expect(s.personalValue).toBe(5_000_000);
  });
});
