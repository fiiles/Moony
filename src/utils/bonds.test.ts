import { describe, expect, it } from 'vitest';
import type { Bond } from '@shared/schema';
import {
  bondCouponDue,
  bondFaceValue,
  bondLadder,
  bondYearlyCoupon,
  bondsSummary,
  isMatured,
  nearestMaturity,
  weightedYearsToMaturity,
} from './bonds';

const day = (iso: string) => {
  const [y, m, d] = iso.split('-').map(Number);
  return Date.UTC(y, m - 1, d) / 1000;
};

function bond(partial: Partial<Bond> & { couponValue: string }): Bond {
  return {
    id: partial.id ?? partial.couponValue,
    name: partial.name ?? 'Bond',
    isin: null,
    quantity: '1',
    currency: 'CZK',
    interestRate: '0',
    maturityDate: null,
    purchaseDate: null,
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}

const TODAY = day('2026-10-02');
/** 1 EUR = 25 CZK; no rate for KWD. */
const convert = (amount: number, currency: string) =>
  currency === 'KWD' ? null : currency === 'EUR' ? amount * 25 : amount;

describe('bond values', () => {
  it('multiplies face value by quantity and applies the yearly rate', () => {
    const b = bond({ couponValue: '10000', quantity: '5', interestRate: '4.9' });
    expect(bondFaceValue(b)).toBe(50_000);
    expect(bondYearlyCoupon(b)).toBeCloseTo(2_450);
  });

  it('treats a maturity before today as matured', () => {
    expect(isMatured(bond({ couponValue: '1', maturityDate: day('2025-02-14') }), TODAY)).toBe(
      true
    );
    expect(isMatured(bond({ couponValue: '1', maturityDate: day('2027-06-15') }), TODAY)).toBe(
      false
    );
    expect(isMatured(bond({ couponValue: '1' }), TODAY)).toBe(false);
  });
});

describe('bondCouponDue', () => {
  // 1 700 000 CZK at 3.75 %: a full year pays 63 750
  const short = (purchaseDate: number | null) =>
    bond({
      couponValue: '1',
      quantity: '1700000',
      interestRate: '3.75',
      maturityDate: day('2027-01-15'),
      purchaseDate,
    });

  it('pays the full yearly coupon when the purchase day is unknown', () => {
    expect(bondCouponDue(short(null), day('2027-01-15'))).toBeCloseTo(63_750);
  });

  it('pays only the days held when the bond was bought inside the coupon year', () => {
    // Bought 2026-10-05: 102 of the 365 days from 2026-01-15 to the maturity
    expect(bondCouponDue(short(day('2026-10-05')), day('2027-01-15'))).toBeCloseTo(
      (63_750 * 102) / 365
    );
  });

  it('pays the full coupon when the bond was bought before the coupon year began', () => {
    expect(bondCouponDue(short(day('2025-12-01')), day('2027-01-15'))).toBeCloseTo(63_750);
  });

  it('pro-rates the first coupon after the purchase and pays later ones in full', () => {
    const long = bond({
      couponValue: '1000',
      quantity: '10',
      interestRate: '5',
      maturityDate: day('2029-03-15'),
      purchaseDate: day('2026-09-15'),
    });
    // 2026-09-15 → 2027-03-15 is 181 of the 365 days of that coupon year
    expect(bondCouponDue(long, day('2027-03-15'))).toBeCloseTo((500 * 181) / 365);
    expect(bondCouponDue(long, day('2028-03-15'))).toBeCloseTo(500);
  });

  it('measures the coupon year of a 29 February maturity from 28 February', () => {
    const leap = bond({
      couponValue: '1000',
      quantity: '1',
      interestRate: '4',
      maturityDate: day('2028-02-29'),
      purchaseDate: day('2027-08-31'),
    });
    // 2027-02-28 → 2028-02-29 is 366 days; held from 2027-08-31: 182 of them
    expect(bondCouponDue(leap, day('2028-02-29'))).toBeCloseTo((40 * 182) / 366);
  });
});

describe('bondLadder', () => {
  const bonds = [
    bond({
      id: 'a',
      couponValue: '10000',
      quantity: '5',
      interestRate: '4.9',
      maturityDate: day('2027-06-15'),
    }),
    bond({
      id: 'b',
      couponValue: '10000',
      quantity: '10',
      interestRate: '4.5',
      maturityDate: day('2030-04-29'),
    }),
    bond({
      id: 'old',
      couponValue: '10000',
      quantity: '6',
      interestRate: '2.4',
      maturityDate: day('2025-02-14'),
    }),
  ];

  it('returns principal in the maturity year and coupons on remaining anniversaries', () => {
    const ladder = bondLadder(bonds, TODAY, convert);
    expect(ladder.map((y) => y.year)).toEqual([2026, 2027, 2028, 2029, 2030]);
    // 2026: both anniversaries (15 Jun, 29 Apr) already passed this year
    expect(ladder[0]).toEqual({ year: 2026, principal: 0, coupons: 0 });
    // 2027: bond a matures (50 000) and pays its last coupon; b pays a coupon
    expect(ladder[1].principal).toBe(50_000);
    expect(ladder[1].coupons).toBeCloseTo(2_450 + 4_500);
    // 2030: bond b matures
    expect(ladder[4].principal).toBe(100_000);
    expect(ladder[4].coupons).toBeCloseTo(4_500);
  });

  it('pays a short bond only the coupon for the days it was held', () => {
    const short = bond({
      id: 's',
      couponValue: '1',
      quantity: '1700000',
      interestRate: '3.75',
      maturityDate: day('2027-01-15'),
      purchaseDate: day('2026-10-02'),
    });
    const ladder = bondLadder([short], TODAY, convert);
    // 2026-10-02 → 2027-01-15 is 105 of 365 days
    expect(ladder[1].principal).toBe(1_700_000);
    expect(ladder[1].coupons).toBeCloseTo((63_750 * 105) / 365);
  });

  it('ignores matured bonds and is empty without live bonds', () => {
    expect(bondLadder([bonds[2]], TODAY, convert)).toEqual([]);
  });

  it('pays perpetual bonds every year of the range', () => {
    const perpetual = bond({ id: 'p', couponValue: '1000', quantity: '10', interestRate: '6' });
    const ladder = bondLadder([bonds[0], perpetual], TODAY, convert);
    expect(ladder.map((y) => y.coupons).map((c) => Math.round(c))).toEqual([600, 3050]);
  });
});

describe('maturity helpers', () => {
  it('weights years to maturity by face value', () => {
    const bonds = [
      bond({ id: 'a', couponValue: '50000', maturityDate: day('2027-10-02') }), // 1 year
      bond({ id: 'b', couponValue: '50000', maturityDate: day('2029-10-02') }), // 3 years
      bond({ id: 'p', couponValue: '999999' }), // no maturity: ignored
    ];
    expect(weightedYearsToMaturity(bonds, TODAY, convert)).toBeCloseTo(2, 1);
    expect(weightedYearsToMaturity([bonds[2]], TODAY, convert)).toBeNull();
  });

  it('finds the nearest live maturity', () => {
    const bonds = [
      bond({ id: 'late', couponValue: '1', maturityDate: day('2030-01-01') }),
      bond({ id: 'soon', couponValue: '1', maturityDate: day('2027-01-01') }),
      bond({ id: 'gone', couponValue: '1', maturityDate: day('2025-01-01') }),
    ];
    expect(nearestMaturity(bonds, TODAY)?.id).toBe('soon');
    expect(nearestMaturity([bonds[2]], TODAY)).toBeNull();
  });
});

describe('bondsSummary', () => {
  it('converts every bond into the main currency', () => {
    const s = bondsSummary(
      [
        bond({ id: 'eur', couponValue: '100', currency: 'EUR', interestRate: '4' }),
        bond({ id: 'czk', couponValue: '500', interestRate: '2' }),
      ],
      convert,
      TODAY
    );
    expect(s.totalValue).toBe(3000);
    expect(s.projectedYearlyIncome).toBeCloseTo(100 + 10);
    expect(s.averageYield).toBeCloseTo((2500 * 4 + 500 * 2) / 3000);
    expect(s.missing).toEqual([]);
  });

  it('excludes unconvertible bonds from the total and reports them', () => {
    const s = bondsSummary(
      [
        bond({ id: 'eur', couponValue: '100', currency: 'EUR' }),
        bond({ id: 'kwd', couponValue: '7', currency: 'KWD', maturityDate: day('2027-01-01') }),
      ],
      convert,
      TODAY
    );
    expect(s.totalValue).toBe(2500);
    expect(s.missing).toEqual(['KWD']);
    expect(s.liveCount).toBe(2);
    // The nearest maturity is the KWD bond: known by date, not convertible
    expect(s.nearest?.id).toBe('kwd');
    expect(s.nearestValue).toBeNull();
    // Without a converted value it carries no weight in the years to maturity
    expect(s.weightedYears).toBeNull();
    expect(bondLadder(s.nearest ? [s.nearest] : [], TODAY, convert)).toEqual([]);
  });
});
