import { describe, expect, it } from 'vitest';
import { twrPercent, twrTickDigits } from './twr';

describe('twrPercent', () => {
  it('reads the start of the period (index 100) as no return', () => {
    expect(twrPercent(100)).toBe(0);
  });

  it('turns an index above 100 into a positive ratio', () => {
    expect(twrPercent(112.4)).toBeCloseTo(0.124, 10);
    expect(twrPercent(200)).toBe(1);
  });

  it('turns an index below 100 into a negative ratio', () => {
    expect(twrPercent(90)).toBeCloseTo(-0.1, 10);
    expect(twrPercent(0)).toBe(-1);
  });

  it('agrees with the cumulative percent the backend sends (index = 100 + twr)', () => {
    // 12.4 means +12.4 % in `TwrDataPoint.twr`; as a ratio for fmt.percent that is 0.124
    for (const twr of [-35.2, -0.5, 0, 0.5, 12.4, 80]) {
      expect(twrPercent(100 + twr)).toBeCloseTo(twr / 100, 10);
    }
  });
});

describe('twrTickDigits', () => {
  it('shows whole percentages without decimals', () => {
    expect(twrTickDigits(100)).toBe(0);
    expect(twrTickDigits(105)).toBe(0);
    expect(twrTickDigits(90)).toBe(0);
  });

  it('adds the digits a half or quarter step needs, so no tick is rounded to a wrong number', () => {
    expect(twrTickDigits(102.5)).toBe(1);
    expect(twrTickDigits(97.5)).toBe(1);
    expect(twrTickDigits(100.2)).toBe(1);
    expect(twrTickDigits(100.25)).toBe(2);
    // The steps of an axis that spans about 0.1 percentage points
    expect(twrTickDigits(100.025)).toBe(3);
    expect(twrTickDigits(100.075)).toBe(3);
  });

  it('ignores floating point noise', () => {
    expect(twrTickDigits(100.30000000000001)).toBe(1);
    expect(twrTickDigits(104.99999999999999)).toBe(0);
  });
});
