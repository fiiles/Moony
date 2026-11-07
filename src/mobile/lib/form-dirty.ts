/**
 * Whether a phone form holds input the person would lose (audit C5): its values differ from the
 * ones it opened with. Forms here are plain data (strings, numbers, booleans, null, arrays and
 * objects of them), compared by value. Nothing typed is one value: an empty or whitespace-only
 * text, `null` and a field not there yet are the same, so a stray space or a field typed into and
 * cleared again does not make a sheet ask before it closes.
 */
export function formChanged(start: unknown, now: unknown): boolean {
  return !same(start, now);
}

function blank(value: unknown): unknown {
  if (value === undefined || value === null) return '';
  return typeof value === 'string' && value.trim() === '' ? '' : value;
}

function same(a: unknown, b: unknown): boolean {
  a = blank(a);
  b = blank(b);
  if (Object.is(a, b)) return true;
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    return a.every((item, i) => same(item, b[i]));
  }
  if (a !== null && b !== null && typeof a === 'object' && typeof b === 'object') {
    const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
    for (const key of keys) {
      if (!same((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key])) {
        return false;
      }
    }
    return true;
  }
  return false;
}
