import { describe, expect, it } from 'vitest';
import { parseAmountInput } from './amount-input';

describe('parseAmountInput', () => {
  it('reads plain numbers', () => {
    expect(parseAmountInput('95300')).toBe('95300');
    expect(parseAmountInput('  1240.5 ')).toBe('1240.5');
    expect(parseAmountInput('0')).toBe('0');
  });

  it('takes a comma as the decimal separator', () => {
    expect(parseAmountInput('1240,50')).toBe('1240.50');
    expect(parseAmountInput(',5')).toBe('0.5');
  });

  it('drops spaces of every kind as thousands separators', () => {
    expect(parseAmountInput('95 300,50')).toBe('95300.50');
    expect(parseAmountInput('95 300')).toBe('95300');
    expect(parseAmountInput('1 234 567')).toBe('1234567');
  });

  it('lets the last separator be the decimal one when both appear', () => {
    expect(parseAmountInput('1.234,56')).toBe('1234.56');
    expect(parseAmountInput('1,234.56')).toBe('1234.56');
  });

  it('keeps a sign, also the typographic minus', () => {
    expect(parseAmountInput('-120,5')).toBe('-120.5');
    expect(parseAmountInput('−120')).toBe('-120');
  });

  it('drops a trailing separator', () => {
    expect(parseAmountInput('12,')).toBe('12');
  });

  it('is null for empty or non-numeric text', () => {
    expect(parseAmountInput('')).toBeNull();
    expect(parseAmountInput('   ')).toBeNull();
    expect(parseAmountInput('abc')).toBeNull();
    expect(parseAmountInput('12abc')).toBeNull();
    expect(parseAmountInput('1,2,3')).toBeNull();
    expect(parseAmountInput('1.2.3')).toBeNull();
    expect(parseAmountInput('-')).toBeNull();
    expect(parseAmountInput('+5')).toBeNull();
  });
});
