/**
 * Sensible defaults for the first-run wizard, derived from the system locale
 */
import { CURRENCIES, type CurrencyCode } from '@shared/currencies';

/** ISO 3166-1 alpha-2 region → default display currency (supported currencies only). */
const REGION_CURRENCY: Record<string, CurrencyCode> = {
  CZ: 'CZK',
  US: 'USD',
  GB: 'GBP',
  JP: 'JPY',
  AU: 'AUD',
  CA: 'CAD',
  CH: 'CHF',
  LI: 'CHF',
  HK: 'HKD',
  CN: 'CNY',
  SE: 'SEK',
  NO: 'NOK',
  DK: 'DKK',
  SG: 'SGD',
  NZ: 'NZD',
  PL: 'PLN',
  HU: 'HUF',
  RO: 'RON',
  BG: 'BGN',
  IS: 'ISK',
  TR: 'TRY',
  IN: 'INR',
  BR: 'BRL',
  MX: 'MXN',
  ZA: 'ZAR',
  KR: 'KRW',
  ID: 'IDR',
  IL: 'ILS',
  MY: 'MYR',
  PH: 'PHP',
  TH: 'THB',
  AE: 'AED',
  SA: 'SAR',
  AR: 'ARS',
  CL: 'CLP',
  CO: 'COP',
  EG: 'EGP',
  KW: 'KWD',
  QA: 'QAR',
  TW: 'TWD',
  UA: 'UAH',
  VN: 'VND',
  RS: 'RSD',
  BH: 'BHD',
  OM: 'OMR',
  PA: 'USD',
  EC: 'USD',
  SV: 'USD',
  PR: 'USD',
};

const EURO_REGIONS = [
  'AT',
  'BE',
  'CY',
  'DE',
  'EE',
  'ES',
  'FI',
  'FR',
  'GR',
  'HR',
  'IE',
  'IT',
  'LT',
  'LU',
  'LV',
  'MT',
  'NL',
  'PT',
  'SI',
  'SK',
  'AD',
  'MC',
  'SM',
  'VA',
  'ME',
  'XK',
];

/** Region subtag of a BCP 47 locale ("cs-CZ" → "CZ"), or null. */
export function regionFromLocale(locale: string | undefined | null): string | null {
  if (!locale) return null;
  try {
    const region = new Intl.Locale(locale).maximize().region;
    return region ? region.toUpperCase() : null;
  } catch {
    const m = /[-_]([A-Za-z]{2})\b/.exec(locale);
    return m ? m[1].toUpperCase() : null;
  }
}

/** Default display currency for a region; EUR for the euro area, else `fallback`. */
export function defaultCurrencyForRegion(
  region: string | null | undefined,
  fallback: CurrencyCode = 'EUR'
): CurrencyCode {
  if (!region) return fallback;
  const upper = region.toUpperCase();
  if (REGION_CURRENCY[upper]) return REGION_CURRENCY[upper];
  if (EURO_REGIONS.includes(upper)) return 'EUR';
  return fallback;
}

/**
 * Ids of the shipped country rule packs (`src-tauri/resources/rules/*.json`,
 * without `global`). Kept here so the first-run wizard can offer a country
 * before the database exists; a test guards against drift.
 */
export const RULE_PACK_COUNTRY_IDS = [
  'ae',
  'at',
  'au',
  'be',
  'bh',
  'ca',
  'ch',
  'cl',
  'cn',
  'cy',
  'cz',
  'de',
  'dk',
  'ee',
  'es',
  'fi',
  'fr',
  'gb',
  'gr',
  'hk',
  'hr',
  'hu',
  'ie',
  'il',
  'is',
  'it',
  'jp',
  'kr',
  'kw',
  'lt',
  'lu',
  'lv',
  'mt',
  'nl',
  'no',
  'nz',
  'om',
  'pa',
  'pl',
  'pt',
  'qa',
  'ro',
  'sa',
  'se',
  'sg',
  'si',
  'sk',
  'tw',
  'us',
  'uy',
] as const;

export interface CountryOption {
  /** Rule pack id, lowercase ISO 3166-1 alpha-2 ("cz"). */
  packId: string;
  /** Localized country name for the UI. */
  name: string;
}

/**
 * Country options from the shipped rule-pack ids (everything except
 * "global"), named in the UI language and sorted by that name.
 */
export function countryOptionsFromPacks(packIds: string[], uiLocale: string): CountryOption[] {
  let names: Intl.DisplayNames | null = null;
  try {
    names = new Intl.DisplayNames([uiLocale], { type: 'region' });
  } catch {
    names = null;
  }
  return packIds
    .filter((id) => id !== 'global' && /^[a-z]{2}$/.test(id))
    .map((id) => ({
      packId: id,
      name: names?.of(id.toUpperCase()) ?? id.toUpperCase(),
    }))
    .sort((a, b) => a.name.localeCompare(b.name, uiLocale));
}

/** Rule pack id for a region when a pack exists for it, else null. */
export function packIdForRegion(
  region: string | null | undefined,
  packIds: string[]
): string | null {
  if (!region) return null;
  const id = region.toLowerCase();
  return packIds.includes(id) ? id : null;
}

/** Whether a currency code is one the app supports for display. */
export function isSupportedCurrency(code: string): code is CurrencyCode {
  return Object.prototype.hasOwnProperty.call(CURRENCIES, code);
}
