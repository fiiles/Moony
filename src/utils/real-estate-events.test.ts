import { describe, expect, it } from 'vitest';
import {
  firstRealEstateDay,
  realEstateEvents,
  type PropertyLike,
  type ValuationLike,
} from './real-estate-events';
import { utcDayStart } from './period';

const BOUGHT = utcDayStart('2019-03-14');
const D1 = utcDayStart('2021-05-01');
const D2 = utcDayStart('2023-09-20');
const CREATED = utcDayStart('2026-10-01');

function property(partial: Partial<PropertyLike> = {}): PropertyLike {
  return {
    id: 'flat',
    name: 'Flat',
    purchasePrice: '5000000',
    purchasePriceCurrency: 'CZK',
    purchaseDate: BOUGHT,
    createdAt: CREATED,
    ...partial,
  };
}

let seq = 0;
function valuation(partial: Partial<ValuationLike> & Pick<ValuationLike, 'valuedAt'>) {
  seq += 1;
  return {
    id: `v${seq}`,
    assetId: 'flat',
    value: '6000000',
    currency: 'CZK',
    createdAt: partial.valuedAt,
    ...partial,
  } satisfies ValuationLike;
}

// Report currency CZK: 1 EUR = 25 CZK on every day
const RATES: Record<string, number> = { CZK: 1, EUR: 25 };
const toCzk = (amount: number, currency: string) =>
  RATES[currency] === undefined ? null : amount * RATES[currency];

describe('realEstateEvents', () => {
  it('marks the purchase at its day with the price converted that day', () => {
    const seen: [number, string, number][] = [];
    const events = realEstateEvents(
      [property({ purchasePrice: '200000', purchasePriceCurrency: 'EUR' })],
      [],
      (amount, currency, date) => {
        seen.push([amount, currency, date]);
        return toCzk(amount, currency);
      }
    );
    expect(events).toEqual([
      {
        id: 'purchase-flat',
        t: BOUGHT,
        type: 'buy',
        kind: 'purchase',
        propertyId: 'flat',
        name: 'Flat',
        native: { amount: 200000, currency: 'EUR' },
        value: 5_000_000,
        change: null,
      },
    ]);
    expect(seen).toEqual([[200000, 'EUR', BOUGHT]]);
  });

  it('has no purchase mark without a day or a price', () => {
    const events = realEstateEvents(
      [
        property({ id: 'a', purchaseDate: null }),
        property({ id: 'b', purchasePrice: '0' }),
        property({ id: 'c', purchasePrice: '' }),
      ],
      [],
      toCzk
    );
    expect(events).toEqual([]);
  });

  it('marks every estimate with its change against the previous one', () => {
    const events = realEstateEvents(
      [property()],
      [
        valuation({ id: 'e1', valuedAt: D1, value: '6000000' }),
        valuation({ id: 'e2', valuedAt: D2, value: '6600000' }),
      ],
      toCzk
    );
    expect(events.map((e) => [e.id, e.kind, e.type, e.t, e.value, e.change])).toEqual([
      ['purchase-flat', 'purchase', 'buy', BOUGHT, 5_000_000, null],
      ['valuation-e1', 'first', 'mark', D1, 6_000_000, null],
      ['valuation-e2', 'revaluation', 'mark', D2, 6_600_000, expect.closeTo(0.1, 10)],
    ]);
  });

  it('drops the initial estimate that repeats the purchase on its own day', () => {
    const events = realEstateEvents(
      [property()],
      [
        valuation({ id: 'initial', valuedAt: BOUGHT, value: '5000000' }),
        valuation({ id: 'later', valuedAt: D1, value: '5500000' }),
      ],
      toCzk
    );
    expect(events.map((e) => e.id)).toEqual(['purchase-flat', 'valuation-later']);
    // The dropped estimate still is the one the next revaluation is measured against
    expect(events[1].kind).toBe('revaluation');
    expect(events[1].change).toBeCloseTo(0.1, 10);
  });

  it('keeps a first estimate that differs from the purchase', () => {
    const base = property();
    const sameDayOtherPrice = valuation({ id: 'a', valuedAt: BOUGHT, value: '5200000' });
    const otherDaySamePrice = valuation({ id: 'b', valuedAt: D1, value: '5000000' });
    const otherCurrency = valuation({
      id: 'c',
      valuedAt: BOUGHT,
      value: '5000000',
      currency: 'EUR',
    });
    expect(realEstateEvents([base], [sameDayOtherPrice], toCzk).map((e) => e.id)).toEqual([
      'purchase-flat',
      'valuation-a',
    ]);
    expect(realEstateEvents([base], [otherDaySamePrice], toCzk).map((e) => e.id)).toEqual([
      'purchase-flat',
      'valuation-b',
    ]);
    expect(realEstateEvents([base], [otherCurrency], toCzk).map((e) => e.id)).toEqual([
      'purchase-flat',
      'valuation-c',
    ]);
  });

  it('keeps the first estimate of a property whose purchase is unknown', () => {
    const events = realEstateEvents(
      [property({ purchaseDate: null })],
      [valuation({ id: 'only', valuedAt: CREATED, value: '5000000' })],
      toCzk
    );
    expect(events.map((e) => [e.id, e.kind])).toEqual([['valuation-only', 'first']]);
  });

  it('marks the newest estimate of a day once', () => {
    const events = realEstateEvents(
      [property({ purchaseDate: null })],
      [
        valuation({ id: 'new', valuedAt: D1, value: '6100000', createdAt: 2000 }),
        valuation({ id: 'old', valuedAt: D1, value: '6000000', createdAt: 1000 }),
      ],
      toCzk
    );
    expect(events.map((e) => e.id)).toEqual(['valuation-new']);
    expect(events[0].value).toBe(6_100_000);
  });

  it('compares estimates in one currency as stored, whatever the rates did', () => {
    // The euro lost half its crown value between the two days: the estimate still rose 10 %
    const rate: Record<number, number> = { [D1]: 25, [D2]: 12.5 };
    const events = realEstateEvents(
      [property({ purchaseDate: null })],
      [
        valuation({ id: 'e1', valuedAt: D1, value: '200000', currency: 'EUR' }),
        valuation({ id: 'e2', valuedAt: D2, value: '220000', currency: 'EUR' }),
      ],
      (amount, _currency, date) => amount * rate[date]
    );
    expect(events.map((e) => e.value)).toEqual([5_000_000, 2_750_000]);
    expect(events[1].change).toBeCloseTo(0.1, 10);
  });

  it('compares estimates across currencies through their converted values', () => {
    const events = realEstateEvents(
      [property({ purchaseDate: null })],
      [
        valuation({ id: 'e1', valuedAt: D1, value: '5000000', currency: 'CZK' }),
        valuation({ id: 'e2', valuedAt: D2, value: '220000', currency: 'EUR' }),
      ],
      toCzk
    );
    expect(events[1].change).toBeCloseTo(0.1, 10);
  });

  it('keeps a mark whose day has no rate, with its native amount and no change', () => {
    const events = realEstateEvents(
      [property({ purchaseDate: null })],
      [
        valuation({ id: 'e1', valuedAt: D1, value: '5000000', currency: 'CZK' }),
        valuation({ id: 'e2', valuedAt: D2, value: '220000', currency: 'XXX' }),
      ],
      toCzk
    );
    expect(events[1]).toMatchObject({
      id: 'valuation-e2',
      value: null,
      native: { amount: 220000, currency: 'XXX' },
      change: null,
    });
  });

  it('has no change against an estimate of zero', () => {
    const events = realEstateEvents(
      [property({ purchaseDate: null })],
      [
        valuation({ id: 'e1', valuedAt: D1, value: '0' }),
        valuation({ id: 'e2', valuedAt: D2, value: '100' }),
      ],
      toCzk
    );
    expect(events[0].value).toBe(0);
    expect(events[1].change).toBeNull();
  });

  it('reads a blank currency as CZK', () => {
    const events = realEstateEvents(
      [property({ purchasePriceCurrency: null })],
      [valuation({ id: 'e1', valuedAt: D1, currency: '' })],
      toCzk
    );
    expect(events.map((e) => e.native.currency)).toEqual(['CZK', 'CZK']);
  });

  it('orders by day across properties, a purchase before an estimate of the same day', () => {
    const events = realEstateEvents(
      [
        property({ id: 'flat', name: 'Flat', purchaseDate: D1 }),
        property({ id: 'house', name: 'House', purchaseDate: BOUGHT }),
      ],
      [
        valuation({ id: 'h1', assetId: 'house', valuedAt: D1, value: '7000000' }),
        valuation({ id: 'f1', assetId: 'flat', valuedAt: D1, value: '5300000' }),
        valuation({ id: 'h2', assetId: 'house', valuedAt: D2, value: '7700000' }),
      ],
      toCzk
    );
    expect(events.map((e) => e.id)).toEqual([
      'purchase-house',
      'purchase-flat',
      'valuation-f1',
      'valuation-h1',
      'valuation-h2',
    ]);
    // Each property is compared with its own previous estimate only
    expect(events.find((e) => e.id === 'valuation-h2')?.change).toBeCloseTo(0.1, 10);
  });

  it('ignores the estimates of a property that is gone', () => {
    const events = realEstateEvents(
      [property()],
      [valuation({ id: 'ghost', assetId: 'deleted', valuedAt: D1 })],
      toCzk
    );
    expect(events.map((e) => e.id)).toEqual(['purchase-flat']);
  });

  it('floors the days to UTC midnight', () => {
    const events = realEstateEvents(
      [property({ purchaseDate: BOUGHT + 3600 })],
      [valuation({ id: 'e1', valuedAt: D1 + 7200 })],
      toCzk
    );
    expect(events.map((e) => e.t)).toEqual([BOUGHT, D1]);
  });
});

describe('firstRealEstateDay', () => {
  it('is the earliest purchase, estimate or creation day', () => {
    expect(
      firstRealEstateDay(
        [
          { purchaseDate: D1, createdAt: CREATED },
          { purchaseDate: null, createdAt: D2 },
        ],
        [{ valuedAt: BOUGHT }]
      )
    ).toBe(BOUGHT);
    expect(firstRealEstateDay([{ purchaseDate: null, createdAt: D2 }], [{ valuedAt: D1 }])).toBe(
      D1
    );
    expect(firstRealEstateDay([{ purchaseDate: D1, createdAt: CREATED }], [{ valuedAt: D2 }])).toBe(
      D1
    );
  });

  it('has no day without a property or an estimate', () => {
    expect(firstRealEstateDay([], [])).toBeUndefined();
  });
});
