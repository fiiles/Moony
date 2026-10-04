/**
 * Search helper for the holdings tables: one text box filters by ticker, name and tag.
 * Matching ignores case and diacritics, so "skoda" finds "Škoda", and every word of the term
 * has to match somewhere in the row ("apple tech" finds Apple tagged Technology).
 */

/** Lower-cases, trims and strips combining marks ("Škoda" -> "skoda"). */
export function normalizeSearchText(text: string): string {
  return text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().trim();
}

/** True when every word of `term` is contained in at least one of the row's text fields. */
export function matchesSearch(
  term: string,
  fields: ReadonlyArray<string | null | undefined>
): boolean {
  const words = normalizeSearchText(term).split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;

  const haystack = fields
    .filter((field): field is string => !!field)
    .map(normalizeSearchText)
    .join('\n');
  return words.every((word) => haystack.includes(word));
}
