import { describe, it, expect } from 'vitest';
import { formatAmountWithCode, formatAmountInCurrency } from './format-amount';

describe('formatAmountWithCode', () => {
  it('uses two decimals for USD and appends the code', () => {
    expect(formatAmountWithCode(1234.5, 'USD', 'en-US')).toBe('1,234.50 USD');
  });

  it('follows the UI locale separators', () => {
    expect(formatAmountWithCode(1234.5, 'USD', 'cs-CZ')).toBe('1\u00a0234,50 USD');
  });

  it('uses zero decimals for CZK and JPY', () => {
    expect(formatAmountWithCode(1500, 'CZK', 'en-US')).toBe('1,500 CZK');
    expect(formatAmountWithCode(1500, 'JPY', 'en-US')).toBe('1,500 JPY');
  });

  it('keeps the sign of negative amounts', () => {
    expect(formatAmountWithCode(-42.1, 'EUR', 'en-US')).toBe('-42.10 EUR');
  });

  it('omits the code when the currency is missing', () => {
    expect(formatAmountWithCode(10, null, 'en-US')).toBe('10.00');
    expect(formatAmountWithCode(10, undefined, 'en-US')).toBe('10.00');
  });

  describe('formatAmountInCurrency', () => {
    it('uses the currency symbol and the locale separators', () => {
      expect(formatAmountInCurrency(1234.5, 'EUR', 'en-US')).toBe('€1,234.50');
      expect(formatAmountInCurrency(1234.5, 'EUR', 'cs-CZ')).toBe('1\u00a0234,50\u00a0€');
      expect(formatAmountInCurrency(1234, 'CZK', 'cs-CZ')).toBe('1\u00a0234\u00a0Kč');
    });

    it('falls back to the code for unknown currencies', () => {
      expect(formatAmountInCurrency(10, 'XYZ1', 'en-US')).toBe('10.00 XYZ1');
    });
  });
});
