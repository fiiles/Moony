import { describe, expect, it } from 'vitest';
import { CURRENCY_CODES } from '@shared/currencies';
import {
  EXCHANGE_CURRENCIES,
  EXCHANGE_NAMES,
  currencyForExchange,
  exchangeName,
  normalizeQuoteCurrency,
  resolveTickerCurrency,
} from './exchange-names';

describe('exchangeName', () => {
  it('maps the Yahoo exchange codes to readable names', () => {
    expect(exchangeName('NMS')).toBe('NASDAQ');
    expect(exchangeName('NGM')).toBe('NASDAQ');
    expect(exchangeName('NCM')).toBe('NASDAQ');
    expect(exchangeName('NYQ')).toBe('NYSE');
    expect(exchangeName('PCX')).toBe('NYSE Arca');
    expect(exchangeName('BTS')).toBe('BATS');
    expect(exchangeName('NEO')).toBe('NEO (Toronto)');
    expect(exchangeName('TOR')).toBe('Toronto');
    expect(exchangeName('LSE')).toBe('London');
    expect(exchangeName('GER')).toBe('Frankfurt / Xetra');
    expect(exchangeName('FRA')).toBe('Frankfurt / Xetra');
    expect(exchangeName('PRA')).toBe('Prague');
    expect(exchangeName('PAR')).toBe('Paris');
    expect(exchangeName('AMS')).toBe('Amsterdam');
    expect(exchangeName('MIL')).toBe('Milan');
    expect(exchangeName('MCE')).toBe('Madrid');
    expect(exchangeName('VIE')).toBe('Vienna');
    expect(exchangeName('SWX')).toBe('SIX Swiss');
    expect(exchangeName('EBS')).toBe('SIX Swiss');
    expect(exchangeName('STO')).toBe('Stockholm');
    expect(exchangeName('CPH')).toBe('Copenhagen');
    expect(exchangeName('OSL')).toBe('Oslo');
    expect(exchangeName('HKG')).toBe('Hong Kong');
    expect(exchangeName('JPX')).toBe('Tokyo');
    expect(exchangeName('ASX')).toBe('ASX');
    expect(exchangeName('NSI')).toBe('India');
    expect(exchangeName('BSE')).toBe('India');
  });

  it('is case- and whitespace-insensitive', () => {
    expect(exchangeName(' nms ')).toBe('NASDAQ');
  });

  it('falls back to the raw code for unknown exchanges', () => {
    expect(exchangeName('XYZ')).toBe('XYZ');
  });

  it('returns an empty string for a missing code', () => {
    expect(exchangeName(undefined)).toBe('');
    expect(exchangeName(null)).toBe('');
    expect(exchangeName('')).toBe('');
  });
});

describe('currencyForExchange', () => {
  it.each([
    ['PRA', 'CZK'],
    ['GER', 'EUR'],
    ['FRA', 'EUR'],
    ['PAR', 'EUR'],
    ['AMS', 'EUR'],
    ['MIL', 'EUR'],
    ['MCE', 'EUR'],
    ['VIE', 'EUR'],
    ['LSE', 'GBP'],
    ['SWX', 'CHF'],
    ['EBS', 'CHF'],
    ['TOR', 'CAD'],
    ['NEO', 'CAD'],
    ['JPX', 'JPY'],
    ['HKG', 'HKD'],
    ['ASX', 'AUD'],
    ['STO', 'SEK'],
    ['CPH', 'DKK'],
    ['OSL', 'NOK'],
    ['NSI', 'INR'],
    ['BSE', 'INR'],
    ['NMS', 'USD'],
    ['NYQ', 'USD'],
  ])('%s trades in %s', (code, currency) => {
    expect(currencyForExchange(code)).toBe(currency);
  });

  it('defaults to USD for unknown or missing exchanges', () => {
    expect(currencyForExchange('XYZ')).toBe('USD');
    expect(currencyForExchange(undefined)).toBe('USD');
  });

  it('only maps to currencies the app supports', () => {
    for (const currency of Object.values(EXCHANGE_CURRENCIES)) {
      expect(CURRENCY_CODES).toContain(currency);
    }
  });

  it('has a currency for every named exchange that is not USD-quoted by default', () => {
    // Every exchange with a name also resolves to a supported currency (default USD)
    for (const code of Object.keys(EXCHANGE_NAMES)) {
      expect(CURRENCY_CODES).toContain(currencyForExchange(code));
    }
  });
});

describe('normalizeQuoteCurrency', () => {
  it('passes supported currencies through unchanged', () => {
    expect(normalizeQuoteCurrency('USD', 190.5)).toEqual({ currency: 'USD', price: 190.5 });
    expect(normalizeQuoteCurrency('GBP', 85.2)).toEqual({ currency: 'GBP', price: 85.2 });
  });

  it('converts GBp (pence) to GBP and divides the price by 100', () => {
    expect(normalizeQuoteCurrency('GBp', 8520)).toEqual({ currency: 'GBP', price: 85.2 });
    expect(normalizeQuoteCurrency('GBX', 150)).toEqual({ currency: 'GBP', price: 1.5 });
  });

  it('converts other minor-unit quotes the same way', () => {
    expect(normalizeQuoteCurrency('ZAc', 2500)).toEqual({ currency: 'ZAR', price: 25 });
    expect(normalizeQuoteCurrency('ILA', 1000)).toEqual({ currency: 'ILS', price: 10 });
  });

  it('keeps the price undefined when none is given', () => {
    expect(normalizeQuoteCurrency('GBp')).toEqual({ currency: 'GBP', price: undefined });
  });

  it('returns null for an unsupported or missing currency', () => {
    expect(normalizeQuoteCurrency('XXX', 10)).toBeNull();
    expect(normalizeQuoteCurrency(undefined, 10)).toBeNull();
    expect(normalizeQuoteCurrency(null)).toBeNull();
    expect(normalizeQuoteCurrency('')).toBeNull();
  });
});

describe('resolveTickerCurrency', () => {
  it('prefers the quote currency over the exchange default', () => {
    expect(resolveTickerCurrency('NMS', 'EUR', 10)).toEqual({ currency: 'EUR', price: 10 });
  });

  it('applies the pence conversion to a GBp quote', () => {
    expect(resolveTickerCurrency('LSE', 'GBp', 8520)).toEqual({ currency: 'GBP', price: 85.2 });
  });

  it('falls back to the exchange table when the quote has no usable currency', () => {
    expect(resolveTickerCurrency('PRA')).toEqual({ currency: 'CZK', price: undefined });
    expect(resolveTickerCurrency('GER', null)).toEqual({ currency: 'EUR', price: undefined });
    expect(resolveTickerCurrency('GER', 'XXX')).toEqual({ currency: 'EUR', price: undefined });
  });

  it('defaults to USD', () => {
    expect(resolveTickerCurrency('XYZ')).toEqual({ currency: 'USD', price: undefined });
    expect(resolveTickerCurrency(undefined)).toEqual({ currency: 'USD', price: undefined });
  });
});
