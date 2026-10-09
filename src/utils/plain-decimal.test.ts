import { describe, expect, it } from 'vitest';
import { plainDecimal } from './plain-decimal';

describe('plainDecimal', () => {
  it('keeps sub-cent precision', () => {
    expect(plainDecimal(0.00001234)).toBe('0.00001234');
  });

  it('never uses exponent notation', () => {
    expect(plainDecimal(0.00000001)).toBe('0.00000001');
    expect(plainDecimal(1e-7)).toBe('0.0000001');
  });

  it('drops float noise and whole-number fractions', () => {
    expect(plainDecimal(0.1 + 0.2)).toBe('0.3');
    expect(plainDecimal(2)).toBe('2');
    expect(plainDecimal(64250.5)).toBe('64250.5');
  });

  it('returns an empty string for non-finite input', () => {
    expect(plainDecimal(NaN)).toBe('');
  });
});
