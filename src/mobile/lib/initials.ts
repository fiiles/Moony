/**
 * Initials for a row chip ("Lidl Praha 4" → "LP", "Netflix" → "NE"): the first letter of the first
 * two words that hold a letter, or two letters of a single word; upper case, any script.
 */
export function initials(text: string): string {
  const words = text
    .split(/\s+/)
    .map((word) => [...word].filter((ch) => /\p{L}/u.test(ch)))
    .filter((letters) => letters.length > 0);
  if (words.length === 0) return '';
  const picked = words.length === 1 ? words[0].slice(0, 2) : [words[0][0], words[1][0]];
  return picked.join('').toUpperCase();
}
