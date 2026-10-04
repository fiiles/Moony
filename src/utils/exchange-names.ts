/**
 * Yahoo Finance exchange codes: display names and default trading currencies.
 *
 * The ticker search returns the raw Yahoo exchange code ("NMS", "NYQ", "PRA"…) and no currency,
 * so the Add Investment dialog and the Stock Monitor search map the code to a readable name
 * and prefill the currency from the exchange. Unknown codes show as they are
 * and default to USD. A quote that does carry a currency wins over the exchange default.
 */

import { CURRENCY_CODES, type CurrencyCode } from '@shared/currencies';

/** Readable exchange names keyed by Yahoo exchange code (upper case). */
export const EXCHANGE_NAMES: Readonly<Record<string, string>> = {
  NMS: 'NASDAQ',
  NGM: 'NASDAQ',
  NCM: 'NASDAQ',
  NYQ: 'NYSE',
  PCX: 'NYSE Arca',
  ASE: 'NYSE American',
  BTS: 'BATS',
  PNK: 'OTC Markets',
  NEO: 'NEO (Toronto)',
  TOR: 'Toronto',
  LSE: 'London',
  GER: 'Frankfurt / Xetra',
  FRA: 'Frankfurt / Xetra',
  PRA: 'Prague',
  PAR: 'Paris',
  AMS: 'Amsterdam',
  BRU: 'Brussels',
  MIL: 'Milan',
  MCE: 'Madrid',
  VIE: 'Vienna',
  HEL: 'Helsinki',
  SWX: 'SIX Swiss',
  EBS: 'SIX Swiss',
  STO: 'Stockholm',
  CPH: 'Copenhagen',
  OSL: 'Oslo',
  HKG: 'Hong Kong',
  JPX: 'Tokyo',
  ASX: 'ASX',
  NSI: 'India',
  BSE: 'India',
};

/**
 * Default trading currency per Yahoo exchange code. Exchanges not listed here (the US ones
 * included) trade in USD. Only codes whose currency is not USD need an entry.
 */
export const EXCHANGE_CURRENCIES: Readonly<Record<string, CurrencyCode>> = {
  PRA: 'CZK',
  GER: 'EUR',
  FRA: 'EUR',
  PAR: 'EUR',
  AMS: 'EUR',
  BRU: 'EUR',
  MIL: 'EUR',
  MCE: 'EUR',
  VIE: 'EUR',
  HEL: 'EUR',
  LSE: 'GBP',
  SWX: 'CHF',
  EBS: 'CHF',
  TOR: 'CAD',
  NEO: 'CAD',
  JPX: 'JPY',
  HKG: 'HKD',
  ASX: 'AUD',
  STO: 'SEK',
  CPH: 'DKK',
  OSL: 'NOK',
  NSI: 'INR',
  BSE: 'INR',
};

const DEFAULT_CURRENCY: CurrencyCode = 'USD';

function normalizeCode(code: string | null | undefined): string {
  return (code ?? '').trim().toUpperCase();
}

/** Readable name for a Yahoo exchange code; the raw code when unknown, '' when missing. */
export function exchangeName(code: string | null | undefined): string {
  const key = normalizeCode(code);
  if (!key) return '';
  return EXCHANGE_NAMES[key] ?? (code ?? '').trim();
}

/** The currency a ticker listed on this exchange usually trades in (USD when unknown). */
export function currencyForExchange(code: string | null | undefined): CurrencyCode {
  return EXCHANGE_CURRENCIES[normalizeCode(code)] ?? DEFAULT_CURRENCY;
}

/**
 * Quotes in a currency's minor unit (pence, South African cents, Israeli agorot) carry a
 * lower-case or alternative code. Maps each to the major currency and the divisor that turns a
 * minor-unit price into a major-unit price.
 */
const MINOR_UNIT_QUOTES: Readonly<Record<string, { currency: CurrencyCode; divisor: number }>> = {
  GBp: { currency: 'GBP', divisor: 100 },
  GBX: { currency: 'GBP', divisor: 100 },
  ZAc: { currency: 'ZAR', divisor: 100 },
  ILA: { currency: 'ILS', divisor: 100 },
};

export interface NormalizedQuote {
  currency: CurrencyCode;
  /** The price in the major currency; undefined when no price was passed in. */
  price: number | undefined;
}

/**
 * Normalizes the currency a quote reports. Minor-unit codes ("GBp") become the major currency
 * ("GBP") with the price divided by 100. Null when the quote reports no currency the app
 * supports — the caller then falls back to the exchange default.
 */
export function normalizeQuoteCurrency(
  currency: string | null | undefined,
  price?: number
): NormalizedQuote | null {
  const code = (currency ?? '').trim();
  if (!code) return null;

  const minor = MINOR_UNIT_QUOTES[code];
  if (minor) {
    return {
      currency: minor.currency,
      price: price === undefined ? undefined : price / minor.divisor,
    };
  }

  const upper = code.toUpperCase();
  if ((CURRENCY_CODES as readonly string[]).includes(upper)) {
    return { currency: upper as CurrencyCode, price };
  }
  return null;
}

/**
 * The currency (and, when a quote price is given, the major-unit price) to prefill after a
 * ticker is picked: the quote's own currency when it has a supported one, otherwise the
 * exchange default. The user can still change it in the currency combobox.
 */
export function resolveTickerCurrency(
  exchange: string | null | undefined,
  quoteCurrency?: string | null,
  quotePrice?: number
): NormalizedQuote {
  return (
    normalizeQuoteCurrency(quoteCurrency, quotePrice) ?? {
      currency: currencyForExchange(exchange),
      price: quotePrice,
    }
  );
}
