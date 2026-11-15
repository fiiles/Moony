import { describe, expect, it } from 'vitest';
import { frequentCurrencies } from './currency-pick';

describe('frequentCurrencies (audit A10)', () => {
  it('puts the chosen currency first, then the euro and the dollar', () => {
    expect(frequentCurrencies('CZK', ['CZK', 'EUR', 'GBP', 'USD'])).toEqual(['CZK', 'EUR', 'USD']);
  });

  it('names each currency once', () => {
    expect(frequentCurrencies('EUR', ['CZK', 'EUR', 'USD'])).toEqual(['EUR', 'USD']);
    expect(frequentCurrencies('USD', ['EUR', 'USD'])).toEqual(['USD', 'EUR']);
  });

  it('keeps to the currencies the picker offers', () => {
    expect(frequentCurrencies('JPY', ['JPY', 'USD'])).toEqual(['JPY', 'USD']);
    expect(frequentCurrencies('XAU', ['CZK', 'EUR'])).toEqual(['EUR']);
  });
});
