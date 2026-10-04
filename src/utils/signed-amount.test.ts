import { describe, expect, it } from 'vitest';
import { formatSignedAmount, MINUS_SIGN } from './signed-amount';

const czk = (abs: number) => `${abs.toLocaleString('cs-CZ', { maximumFractionDigits: 0 })} Kč`;
const usd = (abs: number) => `$${abs.toLocaleString('en-US', { maximumFractionDigits: 0 })}`;

describe('formatSignedAmount', () => {
  it('prefixes a deficit with a real minus sign, never shows it as a positive number', () => {
    const text = formatSignedAmount(-46708, czk);
    expect(text.startsWith(MINUS_SIGN)).toBe(true);
    expect(text.replace(/\s/g, '')).toBe(`${MINUS_SIGN}46708Kč`);
  });

  it('puts the sign before the currency symbol for symbol-first currencies', () => {
    expect(formatSignedAmount(-1234, usd)).toBe(`${MINUS_SIGN}$1,234`);
  });

  it('leaves a surplus or zero unsigned', () => {
    expect(formatSignedAmount(25000, usd)).toBe('$25,000');
    expect(formatSignedAmount(0, usd)).toBe('$0');
  });

  it('does not show a minus sign for an amount that rounds to zero', () => {
    expect(formatSignedAmount(-0.3, usd)).toBe('$0');
  });
});
