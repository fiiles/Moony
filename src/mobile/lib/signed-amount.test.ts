import { describe, expect, it } from 'vitest';
import { signedAmountText } from './signed-amount';

/** A stand-in for `fmt.money`: the amount at the asked decimals and its currency, no separators. */
const money = (amount: number, currency: string, opts?: { decimals?: number }) =>
  `${opts?.decimals === undefined ? amount : amount.toFixed(opts.decimals)} ${currency}`;

describe('signedAmountText', () => {
  it('puts "+ " before a credit and "− " before a debit, on the amount without its own sign', () => {
    expect(signedAmountText(money, { type: 'credit', amount: '1249.5', currency: 'CZK' })).toBe(
      '+ 1249.50 CZK'
    );
    expect(signedAmountText(money, { type: 'debit', amount: '-1249.5', currency: 'CZK' })).toBe(
      '− 1249.50 CZK'
    );
    expect(signedAmountText(money, { type: 'debit', amount: '80', currency: 'EUR' })).toBe(
      '− 80 EUR'
    );
  });

  it('is exact: the minor units of the currency, none on a whole amount', () => {
    expect(signedAmountText(money, { type: 'debit', amount: '2.06', currency: 'EUR' })).toBe(
      '− 2.06 EUR'
    );
    expect(signedAmountText(money, { type: 'debit', amount: '80.00', currency: 'EUR' })).toBe(
      '− 80 EUR'
    );
    expect(signedAmountText(money, { type: 'debit', amount: '129.90', currency: 'CZK' })).toBe(
      '− 129.90 CZK'
    );
    expect(signedAmountText(money, { type: 'credit', amount: '1262', currency: 'JPY' })).toBe(
      '+ 1262 JPY'
    );
  });

  it('formats an amount that is not a number as zero, not "NaN"', () => {
    expect(signedAmountText(money, { type: 'credit', amount: 'abc', currency: 'CZK' })).toBe(
      '+ 0 CZK'
    );
  });

  it('keeps the sign on a zero amount', () => {
    expect(signedAmountText(money, { type: 'debit', amount: '0', currency: 'CZK' })).toBe(
      '− 0 CZK'
    );
  });
});
