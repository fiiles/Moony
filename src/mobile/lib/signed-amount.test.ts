import { describe, expect, it } from 'vitest';
import { maskMoney } from './hide-amounts';
import { signedAmountText } from './signed-amount';

/** A stand-in for `fmt.money`: the amount and its currency, no separators. */
const money = (amount: number, currency: string) => `${amount} ${currency}`;

describe('signedAmountText', () => {
  it('puts "+ " before a credit and "− " before a debit, on the amount without its own sign', () => {
    expect(signedAmountText(money, { type: 'credit', amount: '1249.5', currency: 'CZK' })).toBe(
      '+ 1249.5 CZK'
    );
    expect(signedAmountText(money, { type: 'debit', amount: '-1249.5', currency: 'CZK' })).toBe(
      '− 1249.5 CZK'
    );
    expect(signedAmountText(money, { type: 'debit', amount: '80', currency: 'EUR' })).toBe(
      '− 80 EUR'
    );
  });

  it('keeps the sign when the formatter masks the number (amounts hidden)', () => {
    const masked = (amount: number, currency: string) => maskMoney(money(amount, currency));
    expect(signedAmountText(masked, { type: 'credit', amount: '1249.5', currency: 'CZK' })).toBe(
      '+ •••• CZK'
    );
    expect(signedAmountText(masked, { type: 'debit', amount: '-1249.5', currency: 'CZK' })).toBe(
      '− •••• CZK'
    );
  });

  it('formats an amount that cannot be read as zero', () => {
    expect(signedAmountText(money, { type: 'credit', amount: '', currency: 'CZK' })).toBe(
      '+ 0 CZK'
    );
  });
});
