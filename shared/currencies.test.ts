import { describe, it, expect, beforeEach } from 'vitest';
import {
  CURRENCIES,
  BASE_CURRENCY,
  EXCHANGE_RATES,
  convertToCzK,
  convertFromCzK,
  updateExchangeRates,
  currencyDecimals,
  priceDecimals,
  currencyName,
  currencyCodeSchema,
  hasKnownRate,
  isEcbCurrency,
  CURRENCY_CODES,
  ECB_CURRENCY_CODES,
  CurrencyCode,
} from './currencies';

const ECB_CODES = [
  'EUR',
  'USD',
  'JPY',
  'BGN',
  'CZK',
  'DKK',
  'GBP',
  'HUF',
  'PLN',
  'RON',
  'SEK',
  'CHF',
  'ISK',
  'NOK',
  'TRY',
  'AUD',
  'BRL',
  'CAD',
  'CNY',
  'HKD',
  'IDR',
  'ILS',
  'INR',
  'KRW',
  'MXN',
  'MYR',
  'NZD',
  'PHP',
  'SGD',
  'THB',
  'ZAR',
] as const;
const EXTRA_CODES = [
  'AED',
  'SAR',
  'ARS',
  'CLP',
  'COP',
  'EGP',
  'KWD',
  'QAR',
  'TWD',
  'UAH',
  'VND',
  'RSD',
  'BHD',
  'OMR',
] as const;

describe('CURRENCIES constant', () => {
  it('contains every ECB-published currency and the common non-ECB ones', () => {
    for (const code of [...ECB_CODES, ...EXTRA_CODES]) {
      expect(CURRENCIES).toHaveProperty(code);
    }
    expect(Object.keys(CURRENCIES)).toHaveLength(ECB_CODES.length + EXTRA_CODES.length);
  });

  it('keeps the 15 original currencies', () => {
    for (const code of [
      'CZK',
      'EUR',
      'USD',
      'GBP',
      'JPY',
      'AUD',
      'CAD',
      'CHF',
      'HKD',
      'CNY',
      'SEK',
      'NOK',
      'DKK',
      'SGD',
      'NZD',
    ]) {
      expect(CURRENCIES).toHaveProperty(code);
    }
  });

  it('carries no presentation fields (Intl supplies symbols, names and placement)', () => {
    for (const [code, def] of Object.entries(CURRENCIES)) {
      expect(def.code).toBe(code);
      expect(Object.keys(def).sort()).toEqual(['code', 'decimals']);
    }
  });

  it('declares ISO 4217 minor units, with CZK shown whole as before', () => {
    expect(CURRENCIES.CZK.decimals).toBe(0);
    for (const code of ['JPY', 'KRW', 'ISK', 'CLP', 'VND'] as const) {
      expect(CURRENCIES[code].decimals).toBe(0);
    }
    for (const code of ['BHD', 'KWD', 'OMR'] as const) {
      expect(CURRENCIES[code].decimals).toBe(3);
    }
    for (const code of ['USD', 'EUR', 'PLN', 'INR', 'AED'] as const) {
      expect(CURRENCIES[code].decimals).toBe(2);
    }
  });

  it('agrees with Intl (CLDR) on the minor unit, except where CLDR drops the unused ISO minor unit', () => {
    // CZK is shown whole by design; HUF, IDR, COP and TWD are shown whole too because their
    // subunit is out of use (CLDR cashDigits = 0).
    const ISO_OVER_CLDR = ['CZK', 'HUF', 'IDR', 'COP', 'TWD'];
    for (const def of Object.values(CURRENCIES)) {
      if (ISO_OVER_CLDR.includes(def.code)) continue;
      const intlDecimals = new Intl.NumberFormat('en', {
        style: 'currency',
        currency: def.code,
      }).resolvedOptions().maximumFractionDigits;
      expect(def.decimals, def.code).toBe(intlDecimals);
    }
  });

  it('is accepted by the Intl currency formatter for every code', () => {
    for (const code of CURRENCY_CODES) {
      expect(
        () => new Intl.NumberFormat('en', { style: 'currency', currency: code })
      ).not.toThrow();
    }
  });
});

describe('CURRENCY_CODES', () => {
  it('lists every currency exactly once, sorted', () => {
    expect([...CURRENCY_CODES]).toEqual(Object.keys(CURRENCIES).sort());
    expect(new Set(CURRENCY_CODES).size).toBe(CURRENCY_CODES.length);
  });
});

describe('ECB_CURRENCY_CODES / isEcbCurrency', () => {
  it('is the set the ECB publishes', () => {
    expect([...ECB_CURRENCY_CODES].sort()).toEqual([...ECB_CODES].sort());
  });

  it('tells ECB currencies from the others', () => {
    expect(isEcbCurrency('PLN')).toBe(true);
    expect(isEcbCurrency('AED')).toBe(false);
    expect(isEcbCurrency('nonsense')).toBe(false);
  });
});

describe('currencyCodeSchema', () => {
  it('accepts every supported code and rejects unknown ones', () => {
    expect(currencyCodeSchema.safeParse('PLN').success).toBe(true);
    expect(currencyCodeSchema.safeParse('AED').success).toBe(true);
    expect(currencyCodeSchema.safeParse('XYZ').success).toBe(false);
    expect(currencyCodeSchema.safeParse('czk').success).toBe(false);
  });
});

describe('currencyName', () => {
  it('returns the English name for the en locale', () => {
    expect(currencyName('CZK', 'en')).toBe('Czech Koruna');
    expect(currencyName('EUR', 'en')).toBe('Euro');
  });

  it('returns a capitalized Czech name for the cs locale', () => {
    expect(currencyName('CZK', 'cs')).toBe('Česká koruna');
    expect(currencyName('USD', 'cs')).toBe('Americký dolar');
  });

  it('falls back to the code for codes Intl does not know', () => {
    expect(currencyName('XYZ', 'en')).toBe('XYZ');
  });

  it('falls back to the code for a malformed code or locale', () => {
    expect(currencyName('not a code', 'en')).toBe('not a code');
    expect(currencyName('USD', '!!')).toBe('USD');
  });

  it('names every supported currency (no bare codes in the picker)', () => {
    for (const code of CURRENCY_CODES) {
      expect(currencyName(code, 'en'), code).not.toBe(code);
      expect(currencyName(code, 'cs'), code).not.toBe(code);
    }
  });
});

describe('BASE_CURRENCY constant', () => {
  it('should be CZK', () => {
    expect(BASE_CURRENCY).toBe('CZK');
  });
});

describe('EXCHANGE_RATES', () => {
  it('includes every supported currency with a fallback rate', () => {
    for (const code of CURRENCY_CODES) {
      expect(EXCHANGE_RATES).toHaveProperty(code);
    }
  });

  it('CZK rate is always 1', () => {
    expect(EXCHANGE_RATES.CZK).toBe(1);
  });

  it('all rates are positive numbers', () => {
    for (const rate of Object.values(EXCHANGE_RATES)) {
      expect(typeof rate).toBe('number');
      expect(rate).toBeGreaterThan(0);
    }
  });
});

describe('convertToCzK', () => {
  beforeEach(() => {
    updateExchangeRates({ EUR: 25.0, USD: 23.0, GBP: 29.0 });
  });

  it('converts EUR to CZK using rate', () => {
    expect(convertToCzK(100, 'EUR')).toBeCloseTo(2500, 2);
  });

  it('converts USD to CZK using rate', () => {
    expect(convertToCzK(10, 'USD')).toBeCloseTo(230, 2);
  });

  it('converts GBP to CZK using rate', () => {
    expect(convertToCzK(1, 'GBP')).toBeCloseTo(29, 2);
  });

  it('returns amount unchanged for CZK', () => {
    expect(convertToCzK(500, 'CZK')).toBe(500);
  });

  it('handles zero amount', () => {
    expect(convertToCzK(0, 'EUR')).toBe(0);
  });

  it('handles negative amounts', () => {
    expect(convertToCzK(-100, 'EUR')).toBe(-2500);
  });

  it('handles decimal amounts', () => {
    expect(convertToCzK(1.5, 'USD')).toBeCloseTo(34.5, 2);
  });

  it('handles internal currencies like CNY', () => {
    updateExchangeRates({ CNY: 3.2 });
    expect(convertToCzK(100, 'CNY')).toBeCloseTo(320, 2);
  });
});

describe('convertFromCzK', () => {
  beforeEach(() => {
    updateExchangeRates({ EUR: 25.0, USD: 23.0, GBP: 29.0 });
  });

  it('converts CZK to EUR', () => {
    expect(convertFromCzK(2500, 'EUR')).toBeCloseTo(100, 2);
  });

  it('converts CZK to USD', () => {
    expect(convertFromCzK(230, 'USD')).toBeCloseTo(10, 2);
  });

  it('converts CZK to GBP', () => {
    expect(convertFromCzK(29, 'GBP')).toBeCloseTo(1, 2);
  });

  it('returns amount unchanged for CZK', () => {
    expect(convertFromCzK(500, 'CZK')).toBe(500);
  });

  it('handles zero amount', () => {
    expect(convertFromCzK(0, 'EUR')).toBe(0);
  });

  it('handles negative amounts', () => {
    expect(convertFromCzK(-2500, 'EUR')).toBeCloseTo(-100, 2);
  });

  it('handles decimal amounts', () => {
    expect(convertFromCzK(34.5, 'USD')).toBeCloseTo(1.5, 2);
  });

  it('handles internal currencies like JPY', () => {
    updateExchangeRates({ JPY: 0.15 });
    expect(convertFromCzK(15, 'JPY')).toBeCloseTo(100, 2);
  });
});

describe('updateExchangeRates', () => {
  it('updates EUR rate and affects subsequent conversions', () => {
    updateExchangeRates({ EUR: 30.0 });
    expect(convertToCzK(1, 'EUR')).toBeCloseTo(30, 2);
  });

  it('updates multiple rates at once', () => {
    updateExchangeRates({ EUR: 26.0, USD: 24.0, GBP: 30.0 });
    expect(EXCHANGE_RATES.EUR).toBe(26.0);
    expect(EXCHANGE_RATES.USD).toBe(24.0);
    expect(EXCHANGE_RATES.GBP).toBe(30.0);
  });

  it('CZK rate always stays 1 even if update attempts to change it', () => {
    updateExchangeRates({ CZK: 999 });
    expect(EXCHANGE_RATES.CZK).toBe(1);
  });

  it('preserves existing rates when updating partial rates', () => {
    updateExchangeRates({ EUR: 25.0, USD: 23.0, GBP: 29.0, CNY: 3.2 });
    const previousChf = EXCHANGE_RATES.CHF;
    updateExchangeRates({ EUR: 30.0 });
    expect(EXCHANGE_RATES.CHF).toBe(previousChf);
  });

  it('handles empty update object', () => {
    const currentEur = EXCHANGE_RATES.EUR;
    updateExchangeRates({});
    expect(EXCHANGE_RATES.EUR).toBe(currentEur);
    expect(EXCHANGE_RATES.CZK).toBe(1);
  });

  it('ignores undefined rates in update', () => {
    updateExchangeRates({ EUR: 25.0, USD: undefined });
    expect(EXCHANGE_RATES.EUR).toBe(25.0);
  });

  it('updates internal currency rates', () => {
    updateExchangeRates({ CNY: 3.5, JPY: 0.18, CHF: 27.0 });
    expect(EXCHANGE_RATES.CNY).toBe(3.5);
    expect(EXCHANGE_RATES.JPY).toBe(0.18);
    expect(EXCHANGE_RATES.CHF).toBe(27.0);
  });
});

describe('Currency conversion round-trip', () => {
  beforeEach(() => {
    updateExchangeRates({ EUR: 25.0, USD: 23.0 });
  });

  it('CZK -> EUR -> CZK returns original amount', () => {
    const original = 1000;
    const inEur = convertFromCzK(original, 'EUR');
    const backToCzk = convertToCzK(inEur, 'EUR');
    expect(backToCzk).toBeCloseTo(original, 2);
  });

  it('USD -> CZK -> USD returns original amount', () => {
    const original = 100;
    const inCzk = convertToCzK(original, 'USD');
    const backToUsd = convertFromCzK(inCzk, 'USD');
    expect(backToUsd).toBeCloseTo(original, 2);
  });
});

describe('Cross-rate derivation via CZK', () => {
  beforeEach(() => {
    updateExchangeRates({ EUR: 25.0, USD: 20.0, JPY: 0.16, AUD: 14.0 });
  });

  it('EUR→USD cross-rate equals (EUR/CZK) / (USD/CZK)', () => {
    const eurInCzk = convertToCzK(100, 'EUR'); // 2500 CZK
    const result = convertFromCzK(eurInCzk, 'USD'); // 2500/20 = 125
    expect(result).toBeCloseTo(125, 2);
  });

  it('JPY→AUD cross-rate is accurate', () => {
    const jpyInCzk = convertToCzK(10000, 'JPY'); // 1600 CZK
    const result = convertFromCzK(jpyInCzk, 'AUD'); // 1600/14 ≈ 114.29
    expect(result).toBeCloseTo(1600 / 14, 2);
  });

  it('round-trip for all new currencies is lossless', () => {
    updateExchangeRates({
      AUD: 14.5,
      CAD: 17.0,
      SEK: 2.2,
      NOK: 2.1,
      DKK: 3.4,
      SGD: 17.5,
      NZD: 13.5,
    });
    const codes: CurrencyCode[] = ['AUD', 'CAD', 'SEK', 'NOK', 'DKK', 'SGD', 'NZD'];
    for (const code of codes) {
      const original = 100;
      const inCzk = convertToCzK(original, code);
      const back = convertFromCzK(inCzk, code);
      expect(back).toBeCloseTo(original, 6);
    }
  });
});

describe('currencyDecimals', () => {
  it('returns 0 for CZK — amounts are shown whole', () => {
    expect(currencyDecimals('CZK')).toBe(0);
  });

  it('returns 0 for JPY, which has no minor unit', () => {
    expect(currencyDecimals('JPY')).toBe(0);
  });

  it('returns 2 for currencies with a minor unit', () => {
    expect(currencyDecimals('USD')).toBe(2);
    expect(currencyDecimals('EUR')).toBe(2);
  });

  it('falls back to Intl for codes outside CURRENCIES', () => {
    expect(currencyDecimals('UGX')).toBe(0);
    expect(currencyDecimals('JOD')).toBe(3);
  });

  it('falls back to 2 for unknown or missing codes', () => {
    expect(currencyDecimals('notacur')).toBe(2);
    expect(currencyDecimals(null)).toBe(2);
    expect(currencyDecimals(undefined)).toBe(2);
  });
});

describe('priceDecimals', () => {
  it('drops decimals for CZK at any magnitude', () => {
    expect(priceDecimals(350.5, 'CZK')).toBe(0);
    expect(priceDecimals(2.75, 'CZK')).toBe(0);
  });

  it('keeps 2 decimals for small prices in currencies that use them', () => {
    expect(priceDecimals(350.5, 'USD')).toBe(2);
  });

  it('drops decimals from 1000 up, where the fraction is noise', () => {
    expect(priceDecimals(1000, 'USD')).toBe(0);
    expect(priceDecimals(-1500.25, 'USD')).toBe(0);
  });
});

describe('hasKnownRate', () => {
  it('knows CZK and the built-in fallback currencies', () => {
    expect(hasKnownRate('CZK')).toBe(true);
    expect(hasKnownRate('EUR')).toBe(true);
    expect(hasKnownRate('USD')).toBe(true);
  });

  it('does not pretend to know a rate for a currency that only has the 1:1 placeholder', () => {
    // SAR is not an ECB currency: nothing ever delivers a rate for it.
    expect(EXCHANGE_RATES.SAR).toBe(1);
    expect(hasKnownRate('SAR')).toBe(false);
  });

  it('learns a rate once the backend delivers one', () => {
    expect(hasKnownRate('PLN')).toBe(false);
    updateExchangeRates({ PLN: 5.8 });
    expect(hasKnownRate('PLN')).toBe(true);
    expect(convertToCzK(10, 'PLN')).toBeCloseTo(58, 6);
  });

  it('reports unknown codes as having no rate', () => {
    expect(hasKnownRate('XYZ')).toBe(false);
    expect(hasKnownRate(null)).toBe(false);
    expect(hasKnownRate(undefined)).toBe(false);
  });
});
