import { useContext, useMemo } from 'react';
import { useLanguage } from '@/i18n/I18nProvider';
import { getFormatters, type Formatters } from '@/lib/format';
import { CurrencyContext } from '@/lib/currency';

/**
 * Locale-bound formatters for the current UI language. Use this for
 * native-currency amounts, plain numbers and calendar days; `useCurrency()`
 * stays the entry point for values that must be converted to the display
 * currency first. Amounts in the profile currency use its symbol, any other
 * currency its ISO code.
 */
export function useFormat(): Formatters {
  const { getLocale } = useLanguage();
  const locale = getLocale();
  // Optional read: the profile currency is the only one shown as a symbol.
  const mainCurrency = useContext(CurrencyContext)?.currencyCode;
  return useMemo(() => getFormatters(locale, mainCurrency), [locale, mainCurrency]);
}
