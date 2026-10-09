import { currencyDecimals } from '@shared/currencies';
import { getFormatters } from '@/lib/format';

/**
 * Locale used when a caller does not pass the UI locale. Callers should pass
 * `useFormat().locale` (or `getLocale()`) so separators follow the UI language.
 */
const FALLBACK_LOCALE = 'en-US';

/**
 * Formats an amount in its own currency as "1,234.50 USD" (decimals per
 * currency: CZK/JPY 0, USD/EUR 2). Used by delete-confirmation dialogs, where
 * the user needs to recognise the exact record in the currency it was entered.
 */
export function formatAmountWithCode(
  amount: number,
  currency: string | null | undefined,
  locale: string = FALLBACK_LOCALE
): string {
  const code = currency || '';
  const decimals = currencyDecimals(code);
  // No money is formatted here: no main currency needed.
  const formatted = getFormatters(locale, undefined).number(amount, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
  return code ? `${formatted} ${code}` : formatted;
}

/**
 * Formats an amount in a given currency (symbol placement and separators
 * follow the UI `locale`): "€1,234.56" / "1 234,56 €" / "1 234 Kč" for the
 * main (profile) currency, the ISO code for any other ("EUR 1,234.56"). Falls
 * back to "<number> <code>" for codes Intl rejects.
 */
export function formatAmountInCurrency(
  amount: number,
  currency: string | null | undefined,
  locale: string,
  mainCurrency: string | undefined
): string {
  if (!Number.isFinite(amount)) return '';
  return getFormatters(locale, mainCurrency).money(amount, currency ?? '');
}
