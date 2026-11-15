import { createContext, useContext, useMemo } from 'react';
import { useLanguage } from '@/i18n/I18nProvider';
import { getFormatters, type Formatters, type FormatStyle } from '@/lib/format';
import { CurrencyContext } from '@/lib/currency';

/** Which shell writes: the phone shell provides `phone` (its date table, README §13). */
export const FormatStyleContext = createContext<FormatStyle>('desktop');

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
  const style = useContext(FormatStyleContext);
  return useMemo(() => getFormatters(locale, mainCurrency, style), [locale, mainCurrency, style]);
}
