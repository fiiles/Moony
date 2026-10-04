import { describe, expect, it } from 'vitest';
import fixture from '../../shared/fixtures/interest-tiers.json';
import {
  calculateEffectiveRate,
  calculateZonedInterest,
  zoneUpperBound,
  type ZoneTier,
} from './bank-account-zones';

// The same cases are read by the Rust service (services/interest_tiers.rs), so
// this mirror and the backend cannot drift apart unnoticed.
describe('calculateZonedInterest — cross-check with the backend', () => {
  it('has cases to check', () => {
    expect(fixture.cases.length).toBeGreaterThan(0);
  });

  for (const testCase of fixture.cases) {
    it(testCase.name, () => {
      const tiers: ZoneTier[] = testCase.tiers;
      expect(calculateZonedInterest(testCase.balance, tiers)).toBeCloseTo(
        testCase.yearlyInterest,
        6
      );
    });
  }
});

describe('calculateEffectiveRate', () => {
  it('is the yearly interest as a percentage of the balance', () => {
    // audit example: 5 000 on 150 000
    const tiers: ZoneTier[] = [
      { fromAmount: '0', toAmount: '100000', interestRate: '5' },
      { fromAmount: '200000', toAmount: null, interestRate: '1' },
    ];
    expect(calculateEffectiveRate(150000, tiers)).toBeCloseTo((5000 / 150000) * 100, 9);
  });

  it('is 0 without tiers or balance', () => {
    expect(calculateEffectiveRate(1000, [])).toBe(0);
    expect(
      calculateEffectiveRate(0, [{ fromAmount: '0', toAmount: null, interestRate: '4' }])
    ).toBe(0);
  });
});

describe('zoneUpperBound', () => {
  it('reads a missing, empty or 0 upper bound as unlimited', () => {
    expect(zoneUpperBound({ toAmount: null })).toBeNull();
    expect(zoneUpperBound({ toAmount: '' })).toBeNull();
    expect(zoneUpperBound({ toAmount: '0' })).toBeNull();
    expect(zoneUpperBound({ toAmount: '100000' })).toBe(100000);
  });
});
