import { describe, it, expect } from 'vitest';
import {
  CURRENCIES,
  currencyDecimals,
  isoMinorUnits,
  priceDecimals,
  currencyName,
  currencyCodeSchema,
  CURRENCY_CODES,
  FX_SOURCE_CODES,
  FX_SUPPORTED_CODES,
  isFxSupported,
} from './currencies';

const ECB_CODES = [
  'EUR',
  'USD',
  'JPY',
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
  'BGN',
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

describe('FX_SOURCE_CODES', () => {
  it('quotes exactly the supported currencies the ECB publishes', () => {
    const quoted = FX_SOURCE_CODES.filter((c) => (CURRENCY_CODES as readonly string[]).includes(c));
    expect([...quoted].sort()).toEqual([...ECB_CODES].sort());
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

describe('isoMinorUnits', () => {
  it('follows the shared ISO table, not the UI cash style', () => {
    expect(isoMinorUnits('CZK')).toBe(2);
    expect(isoMinorUnits(' jod ')).toBe(3);
    expect(isoMinorUnits('UGX')).toBe(0);
    expect(isoMinorUnits('CLF')).toBe(4);
    expect(isoMinorUnits('XAU')).toBe(2);
    expect(isoMinorUnits(null)).toBe(2);
  });
});

describe('fx supported currencies', () => {
  it('has 36 codes: 30 quoted + 6 derived', () => {
    expect(FX_SUPPORTED_CODES).toHaveLength(36);
  });
  it('treats pegs and BGN as supported, floating non-ECB codes as unsupported', () => {
    for (const c of ['AED', 'SAR', 'QAR', 'BHD', 'OMR', 'BGN', 'EUR', 'CZK'])
      expect(isFxSupported(c)).toBe(true);
    for (const c of ['KWD', 'UAH', 'VND', 'ARS', 'TWD']) expect(isFxSupported(c)).toBe(false);
  });
  it('no longer lists BGN as ECB-quoted', () => {
    expect(FX_SOURCE_CODES).not.toContain('BGN');
  });
});
