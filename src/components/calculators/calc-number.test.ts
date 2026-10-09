import { describe, expect, it } from 'vitest';
import { parseCalcNumber, roundSignificant, scaledDefault } from './calc-number';

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

describe('roundSignificant', () => {
  it('keeps two significant digits by default', () => {
    expect(roundSignificant(5_436_123)).toBe(5_400_000);
    expect(roundSignificant(861.4)).toBe(860);
    expect(roundSignificant(0)).toBe(0);
    expect(roundSignificant(-12_345)).toBe(-12_000);
  });
});

describe('scaledDefault', () => {
  const format = (v: number) => String(v);

  it('is the EUR constant itself when the main currency is EUR', () => {
    expect(scaledDefault(140_000, (amount) => amount, format)).toBe('140000');
  });

  it('converts the EUR constant and rounds it to a round amount', () => {
    // 140 000 EUR at 24.7 CZK = 3 458 000 → 3 500 000
    expect(scaledDefault(140_000, (amount) => amount * 24.7, format)).toBe('3500000');
  });

  it('starts empty when no rate is known (never 1:1)', () => {
    expect(scaledDefault(140_000, () => null, format)).toBe('');
  });
});
