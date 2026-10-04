import { describe, expect, it } from 'vitest';
import { allocationPercents } from './allocation-percent';

const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);

describe('allocationPercents', () => {
  it('adds up to exactly 100 where per-item rounding gives 99', () => {
    // [33.4, 33.3, 33.3] % rounds to 33 + 33 + 33 = 99
    const result = allocationPercents([334, 333, 333]);
    expect(sum(result)).toBe(100);
    expect(result).toEqual([34, 33, 33]);
  });

  it('adds up to exactly 100 where per-item rounding gives 103', () => {
    // [12.5 x 5, 37.5] % rounds to 13 x 5 + 38 = 103
    const result = allocationPercents([125, 125, 125, 125, 125, 375]);
    expect(sum(result)).toBe(100);
    // the biggest class wins a tie on the remainder
    expect(result[5]).toBe(38);
    expect(result.every((p) => Number.isInteger(p))).toBe(true);
  });

  it('keeps every share within one point of its exact value', () => {
    const values = [123.4, 56.7, 890.1, 2.3, 45.6, 0.5];
    const total = sum(values);
    const result = allocationPercents(values);
    expect(sum(result)).toBe(100);
    result.forEach((p, i) => {
      expect(Math.abs(p - (values[i] / total) * 100)).toBeLessThan(1);
    });
  });

  it('never gives a share to a class without value', () => {
    expect(allocationPercents([1, 1, 1, 0])).toEqual([34, 33, 33, 0]);
    expect(allocationPercents([100, 0, 0])).toEqual([100, 0, 0]);
  });

  it('is zero everywhere when there is nothing to allocate', () => {
    expect(allocationPercents([0, 0, 0])).toEqual([0, 0, 0]);
    expect(allocationPercents([])).toEqual([]);
  });

  it('ignores negative and non-finite values', () => {
    expect(allocationPercents([50, -20, NaN, 50])).toEqual([50, 0, 0, 50]);
  });

  it('supports decimals and still adds up', () => {
    const result = allocationPercents([1, 1, 1], 1);
    expect(result).toEqual([33.4, 33.3, 33.3]);
    expect(Math.round(sum(result) * 10) / 10).toBe(100);
  });

  it('gives equal classes the same share when they divide evenly', () => {
    expect(allocationPercents([1, 1, 1, 1])).toEqual([25, 25, 25, 25]);
  });
});
