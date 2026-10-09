import { z } from 'zod';
import isoMinorUnitTable from './iso4217-minor-units.json';
import fxSupportedTable from './fx-supported.json';

/**
 * Every currency the app supports — each is both a position currency and a
 * display currency. 30 currencies with an ECB rate, 6 derived from a fixed peg
 * (see `fx-supported.json`), the rest without automatic rates (see
 * `isFxSupported`). Sorted by code.
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

const isCurrencyCode = (code: string): code is CurrencyCode =>
  (CURRENCY_CODES as readonly string[]).includes(code);

/** Codes quoted directly in `fx-supported.json` (the ECB publishes a daily rate for them). */
export const FX_SOURCE_CODES: readonly string[] = fxSupportedTable.source;

/** Codes derived from a fixed peg to an anchor in `fx-supported.json` (computed at read time only). */
export const FX_DERIVED_CODES: readonly string[] = Object.keys(fxSupportedTable.derived);

/** Source ∪ derived codes, restricted to `CURRENCY_CODES`, sorted. */
export const FX_SUPPORTED_CODES: readonly CurrencyCode[] = [...FX_SOURCE_CODES, ...FX_DERIVED_CODES]
  .filter(isCurrencyCode)
  .sort();

const FX_SUPPORTED_SET: ReadonlySet<string> = new Set(FX_SUPPORTED_CODES);

/** Whether a currency has a conversion path (quoted or pegged). False for codes without any rate. */
export function isFxSupported(code: string | null | undefined): boolean {
  return !!code && FX_SUPPORTED_SET.has(code);
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
 * ISO 4217 minor units of the money contract (backend `reporting::minor_units`
 * reads the same table). Reports show this precision so rows, totals and CSV
 * reconcile; `currencyDecimals` stays the everyday cash style (CZK 0).
 */
export function isoMinorUnits(code: string | null | undefined): number {
  const table: Record<string, number | undefined> = isoMinorUnitTable.exceptions;
  const units = code ? table[code.trim().toUpperCase()] : undefined;
  return units ?? isoMinorUnitTable.defaultMinorUnits;
}

/**
 * Decimals for a per-unit price: the currency's own decimals, further cut to 0
 * from 1000 up, where the fraction is noise.
 */
export function priceDecimals(value: number, code: string | null | undefined): number {
  return Math.abs(value) >= 1000 ? 0 : currencyDecimals(code);
}
