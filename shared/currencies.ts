import { z } from 'zod';

/**
 * Every currency the app supports — each is both a position currency and a
 * display currency. The 31 currencies the ECB publishes a daily rate for, plus
 * common currencies it does not (their rates are not updated automatically, see
 * `isEcbCurrency` / `hasKnownRate`). Sorted by code.
 *
 * Symbols, symbol placement and separators come from `Intl` (see
 * `src/lib/format.ts`), names from `currencyName()` — nothing presentational
 * lives here.
 */
export const CURRENCY_CODES = [
  'AED',
  'ARS',
  'AUD',
  'BGN',
  'BHD',
  'BRL',
  'CAD',
  'CHF',
  'CLP',
  'CNY',
  'COP',
  'CZK',
  'DKK',
  'EGP',
  'EUR',
  'GBP',
  'HKD',
  'HUF',
  'IDR',
  'ILS',
  'INR',
  'ISK',
  'JPY',
  'KRW',
  'KWD',
  'MXN',
  'MYR',
  'NOK',
  'NZD',
  'OMR',
  'PHP',
  'PLN',
  'QAR',
  'RON',
  'RSD',
  'SAR',
  'SEK',
  'SGD',
  'THB',
  'TRY',
  'TWD',
  'UAH',
  'USD',
  'VND',
  'ZAR',
] as const;

export type CurrencyCode = (typeof CURRENCY_CODES)[number];

export interface CurrencyDef {
  code: CurrencyCode;
  /** Decimals shown by default. 0 where the minor unit is out of circulation (CZK) or absent (JPY), 3 for BHD/KWD/OMR. */
  decimals: number;
}

/** ISO 4217 minor units that differ from the default of 2 (CZK: 0 as before, the heller is out of circulation). */
const MINOR_UNITS: Partial<Record<CurrencyCode, number>> = {
  CZK: 0,
  CLP: 0,
  ISK: 0,
  JPY: 0,
  KRW: 0,
  VND: 0,
  // ISO 4217 lists 2 minor units, but the subunit is not used in practice (CLDR cashDigits = 0)
  HUF: 0,
  IDR: 0,
  COP: 0,
  TWD: 0,
  BHD: 3,
  KWD: 3,
  OMR: 3,
};

export const CURRENCIES = Object.fromEntries(
  CURRENCY_CODES.map((code) => [code, { code, decimals: MINOR_UNITS[code] ?? 2 }])
) as Record<CurrencyCode, CurrencyDef>;

/** Currencies the ECB publishes a daily reference rate for (rates refresh automatically). */
export const ECB_CURRENCY_CODES: readonly CurrencyCode[] = [
  'AUD',
  'BGN',
  'BRL',
  'CAD',
  'CHF',
  'CNY',
  'CZK',
  'DKK',
  'EUR',
  'GBP',
  'HKD',
  'HUF',
  'IDR',
  'ILS',
  'INR',
  'ISK',
  'JPY',
  'KRW',
  'MXN',
  'MYR',
  'NOK',
  'NZD',
  'PHP',
  'PLN',
  'RON',
  'SEK',
  'SGD',
  'THB',
  'TRY',
  'USD',
  'ZAR',
];

/** Whether the ECB publishes a rate for this currency (false for AED, SAR, ...: no automatic rate). */
export function isEcbCurrency(code: string | null | undefined): boolean {
  return !!code && (ECB_CURRENCY_CODES as readonly string[]).includes(code);
}

const displayNamesByLocale = new Map<string, Intl.DisplayNames | null>();

/**
 * Localized currency name from `Intl.DisplayNames` ("Czech Koruna" / "Česká koruna"),
 * first letter capitalized for list display. Falls back to the code itself when Intl
 * does not know the code or the locale.
 */
export function currencyName(code: string, locale: string): string {
  let names = displayNamesByLocale.get(locale);
  if (names === undefined) {
    try {
      names = new Intl.DisplayNames([locale], { type: 'currency' });
    } catch {
      names = null;
    }
    displayNamesByLocale.set(locale, names);
  }
  if (!names) return code;
  try {
    const name = names.of(code);
    if (!name || name === code) return code;
    return name.charAt(0).toLocaleUpperCase(locale) + name.slice(1);
  } catch {
    return code;
  }
}

/**
 * Zod schema for a supported currency code. Derived from CURRENCIES so form
 * validation can never be narrower than the currency selects (which list every
 * entry). The message is an i18n key resolved by FormMessage (common namespace).
 */
export const currencyCodeSchema = z.enum(
  Object.keys(CURRENCIES) as [CurrencyCode, ...CurrencyCode[]],
  { error: 'validation.currencyInvalid' }
);

// Base currency is CZK
export const BASE_CURRENCY: CurrencyCode = 'CZK';

// Built-in fallback rates (1 unit of currency = X CZK), mirroring the Rust side
// (`services/currency.rs`). The ECB refresh overwrites them on unlock.
const BUILT_IN_RATES: Partial<Record<CurrencyCode, number>> = {
  CZK: 1,
  EUR: 25.0,
  USD: 23.0,
  GBP: 29.0,
  JPY: 0.15,
  AUD: 14.5,
  CAD: 17.0,
  CHF: 26.0,
  HKD: 3.0,
  CNY: 3.2,
  SEK: 2.2,
  NOK: 2.1,
  DKK: 3.4,
  SGD: 17.5,
  NZD: 13.5,
};

/**
 * Exchange rates (1 unit of currency = X CZK). Currencies without a built-in
 * rate hold a 1:1 placeholder (like the backend's last-resort fallback) until a
 * rate arrives through `updateExchangeRates`; `hasKnownRate` tells the two
 * apart, so callers can flag a placeholder instead of showing it as a real
 * conversion.
 */
export const EXCHANGE_RATES = Object.fromEntries(
  CURRENCY_CODES.map((code) => [code, BUILT_IN_RATES[code] ?? 1])
) as Record<CurrencyCode, number>;

const knownRateCodes = new Set<string>(Object.keys(BUILT_IN_RATES));

export function updateExchangeRates(rates: Partial<Record<CurrencyCode, number>>): void {
  Object.entries(rates).forEach(([currency, rate]) => {
    if (rate !== undefined) {
      EXCHANGE_RATES[currency as CurrencyCode] = rate;
      knownRateCodes.add(currency);
    }
  });
  EXCHANGE_RATES.CZK = 1;
}

/**
 * Whether `code` has a real rate (built-in fallback or delivered by the backend)
 * rather than the 1:1 placeholder. False for non-ECB currencies such as AED and
 * for ECB currencies before the first rate load.
 */
export function hasKnownRate(code: string | null | undefined): boolean {
  return !!code && knownRateCodes.has(code);
}

/**
 * Decimals to display for a currency code — including codes outside CURRENCIES
 * (a stock may be listed in PLN, HUF, ...), where Intl knows the minor unit.
 */
export function currencyDecimals(code: string | null | undefined): number {
  if (!code) return 2;
  const def = (CURRENCIES as Record<string, CurrencyDef | undefined>)[code];
  if (def) return def.decimals;
  try {
    return (
      new Intl.NumberFormat('en', { style: 'currency', currency: code }).resolvedOptions()
        .maximumFractionDigits ?? 2
    );
  } catch {
    return 2;
  }
}

/**
 * Decimals for a per-unit price: the currency's own decimals, further cut to 0
 * from 1000 up, where the fraction is noise.
 */
export function priceDecimals(value: number, code: string | null | undefined): number {
  return Math.abs(value) >= 1000 ? 0 : currencyDecimals(code);
}

export function convertToCzK(amount: number, fromCurrency: CurrencyCode): number {
  const rate = EXCHANGE_RATES[fromCurrency];
  return amount * rate;
}

export function convertFromCzK(amountInCzk: number, toCurrency: CurrencyCode): number {
  const rate = EXCHANGE_RATES[toCurrency];
  return amountInCzk / rate;
}
