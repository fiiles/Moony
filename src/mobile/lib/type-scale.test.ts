import { describe, expect, it } from 'vitest';
import { typeScaleFrom } from './type-scale';

describe('typeScaleFrom', () => {
  it('is 1 at the default text size (17 px body)', () => {
    expect(typeScaleFrom(17)).toBe(1);
  });

  it('is the body size over 17, rounded to 3 decimals', () => {
    expect(typeScaleFrom(23)).toBe(1.353);
    expect(typeScaleFrom(14)).toBe(0.824);
    expect(typeScaleFrom(53)).toBe(3.118);
  });

  it('falls back to 1 for a size that is not a positive number', () => {
    expect(typeScaleFrom(Number.NaN)).toBe(1);
    expect(typeScaleFrom(0)).toBe(1);
    expect(typeScaleFrom(-17)).toBe(1);
    expect(typeScaleFrom(Number.POSITIVE_INFINITY)).toBe(1);
  });
});
