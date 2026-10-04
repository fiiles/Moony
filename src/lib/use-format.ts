import { useMemo } from 'react';
import { useLanguage } from '@/i18n/I18nProvider';
import { getFormatters, type Formatters } from '@/lib/format';

/**
 * Locale-bound formatters for the current UI language. Use this for
 * native-currency amounts, plain numbers and calendar days; `useCurrency()`
 * stays the entry point for values that must be converted to the display
 * currency first.
 */
export function useFormat(): Formatters {
  const { getLocale } = useLanguage();
  const locale = getLocale();
  return useMemo(() => getFormatters(locale), [locale]);
}
