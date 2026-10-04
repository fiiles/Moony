/**
 * Search ranking for the currency picker (`CurrencyCombobox`): match a typed
 * query against a currency's code and its names, ignoring case and diacritics
 * ("ceska" finds "Česká koruna"). Pure so it can be unit-tested.
 */

/** Lower-case, trim and strip diacritics. */
export function normalizeSearch(text: string): string {
  return text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().trim();
}

/**
 * Score in [0, 1]; 0 means "does not match" (cmdk hides the item). Ranking:
 * exact code > code prefix > name word start > name substring > code substring.
 * `names` holds every searchable name (localized and English).
 */
export function currencyMatchScore(code: string, names: readonly string[], query: string): number {
  const q = normalizeSearch(query);
  if (!q) return 1;

  const lowerCode = normalizeSearch(code);
  if (lowerCode === q) return 1;
  if (lowerCode.startsWith(q)) return 0.9;

  let best = 0;
  for (const name of names) {
    const n = normalizeSearch(name);
    if (n.split(/[\s-]+/).some((word) => word.startsWith(q))) best = Math.max(best, 0.8);
    else if (n.includes(q)) best = Math.max(best, 0.5);
  }
  if (best > 0) return best;

  return lowerCode.includes(q) ? 0.4 : 0;
}
