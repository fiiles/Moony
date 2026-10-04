import { describe, expect, it } from 'vitest';
import { parseCalcNumber } from './calc-number';

describe('parseCalcNumber', () => {
  it('reads Czech input: space grouping, comma or dot decimals', () => {
    expect(parseCalcNumber('3 500 000', 'cs-CZ')).toBe(3_500_000);
    expect(parseCalcNumber('5 400 000', 'cs-CZ')).toBe(5_400_000);
    expect(parseCalcNumber('4,1', 'cs-CZ')).toBe(4.1);
    expect(parseCalcNumber('4,125', 'cs-CZ')).toBe(4.125);
    expect(parseCalcNumber('4.1', 'cs-CZ')).toBe(4.1);
    expect(parseCalcNumber('1 234,5', 'cs-CZ')).toBe(1234.5);
  });

  it('reads English input: comma grouping, dot decimals', () => {
    expect(parseCalcNumber('5,400,000', 'en-US')).toBe(5_400_000);
    expect(parseCalcNumber('900,000', 'en-US')).toBe(900_000);
    expect(parseCalcNumber('1,234.5', 'en-US')).toBe(1234.5);
    expect(parseCalcNumber('4.1', 'en-US')).toBe(4.1);
    expect(parseCalcNumber('3 500 000', 'en-US')).toBe(3_500_000);
  });

  it('keeps a lone comma that is not a thousands group as the decimal separator', () => {
    expect(parseCalcNumber('4,1', 'en-US')).toBe(4.1);
    expect(parseCalcNumber('0,5', 'en-US')).toBe(0.5);
  });

  it('reads anything unreadable as 0', () => {
    expect(parseCalcNumber('', 'cs-CZ')).toBe(0);
    expect(parseCalcNumber('abc', 'en-US')).toBe(0);
  });
});
