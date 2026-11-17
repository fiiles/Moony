/** Languages whose nouns keep their capital letter inside a phrase. */
const CAPITALISED_NOUNS = new Set(['de']);

/**
 * A breadcrumb's descriptor after the current page ("Nastavení / synchronizace"): lower case, as
 * the design system writes it, except in a language that capitalises nouns, where lowering would
 * misspell them ("Einstellungen / Synchronisierung", never "synchronisierung").
 */
export function breadcrumbTag(label: string, locale: string): string {
  const language = locale.split('-')[0]?.toLowerCase() ?? '';
  return CAPITALISED_NOUNS.has(language) ? label : label.toLocaleLowerCase(locale);
}
