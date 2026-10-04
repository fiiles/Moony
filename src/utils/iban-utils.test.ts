import { describe, it, expect } from 'vitest';
import { isCzechIBAN, ibanToBBAN, formatAccountNumber } from './iban-utils';

describe('isCzechIBAN', () => {
  it('returns true for a valid Czech IBAN', () => {
    expect(isCzechIBAN('CZ6508000000192000145399')).toBe(true);
  });

  it('returns true for Czech IBAN with spaces', () => {
    expect(isCzechIBAN('CZ65 0800 0000 1920 0014 5399')).toBe(true);
  });

  it('returns true for lowercase cz prefix', () => {
    expect(isCzechIBAN('cz6508000000192000145399')).toBe(true);
  });

  it('returns false for non-Czech IBAN', () => {
    expect(isCzechIBAN('DE89370400440532013000')).toBe(false);
  });

  it('returns false for null', () => {
    expect(isCzechIBAN(null)).toBe(false);
  });

  it('returns false for empty string', () => {
    expect(isCzechIBAN('')).toBe(false);
  });

  it('returns false for undefined', () => {
    expect(isCzechIBAN(undefined)).toBe(false);
  });
});

describe('ibanToBBAN', () => {
  it('converts Czech IBAN to account/bank format with prefix', () => {
    // CZ6508000000192000145399: bank=0800, prefix=000019, account=2000145399
    // prefix stripped of leading zeros = 19
    expect(ibanToBBAN('CZ6508000000192000145399')).toBe('19-2000145399/0800');
  });

  it('omits prefix when it is all zeros', () => {
    // CZ5503000000000000123456: bank=0300, prefix=000000, account=0000123456
    expect(ibanToBBAN('CZ5503000000000000123456')).toBe('123456/0300');
  });

  it('handles spaces in IBAN input', () => {
    expect(ibanToBBAN('CZ65 0800 0000 1920 0014 5399')).toBe('19-2000145399/0800');
  });

  it('returns null for non-Czech IBAN', () => {
    expect(ibanToBBAN('DE89370400440532013000')).toBeNull();
  });

  it('returns null for null input', () => {
    expect(ibanToBBAN(null)).toBeNull();
  });

  it('returns null for Czech IBAN with wrong length', () => {
    expect(ibanToBBAN('CZ123')).toBeNull();
  });
});

describe('formatAccountNumber', () => {
  it('returns null for null input', () => {
    expect(formatAccountNumber(null)).toBeNull();
  });

  it('formats non-Czech IBAN with spaces every 4 chars', () => {
    const result = formatAccountNumber('DE89370400440532013000');
    expect(result).toBe('DE89 3704 0044 0532 0130 00');
  });

  it('returns IBAN format (not BBAN) when preferBBAN is false', () => {
    const result = formatAccountNumber('CZ6508000000192000145399', false);
    expect(result).toBe('CZ65 0800 0000 1920 0014 5399');
  });

  it('returns BBAN format when preferBBAN is true and input is Czech IBAN', () => {
    const result = formatAccountNumber('CZ6508000000192000145399', true);
    expect(result).toBe('19-2000145399/0800');
  });
});
