import { describe, expect, it } from 'vitest';
import type { Bond } from '@shared/schema';
import {
  bondFaceValue,
  bondLadder,
  bondYearlyCoupon,
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
    createdAt: 0,
    updatedAt: 0,
    ...partial,
  };
}

const TODAY = day('2026-10-02');

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
    const ladder = bondLadder(bonds, TODAY);
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

  it('ignores matured bonds and is empty without live bonds', () => {
    expect(bondLadder([bonds[2]], TODAY)).toEqual([]);
  });

  it('pays perpetual bonds every year of the range', () => {
    const perpetual = bond({ id: 'p', couponValue: '1000', quantity: '10', interestRate: '6' });
    const ladder = bondLadder([bonds[0], perpetual], TODAY);
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
    expect(weightedYearsToMaturity(bonds, TODAY)).toBeCloseTo(2, 1);
    expect(weightedYearsToMaturity([bonds[2]], TODAY)).toBeNull();
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
