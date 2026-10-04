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
  const formatted = getFormatters(locale).number(amount, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
  return code ? `${formatted} ${code}` : formatted;
}

/**
 * Formats an amount in a given currency (symbol placement and separators
 * follow the UI `locale`): "€1,234.56" / "1 234,56 €" / "1 234 Kč". Falls back
 * to "<number> <code>" for codes Intl rejects.
 */
export function formatAmountInCurrency(
  amount: number,
  currency: string | null | undefined,
  locale: string = FALLBACK_LOCALE
): string {
  if (!Number.isFinite(amount)) return '';
  return getFormatters(locale).money(amount, currency ?? '');
}
