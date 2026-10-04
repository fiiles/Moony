import { getFormatters } from '@/lib/format';

/** The locale's thousands separator ("," in English, a no-break space in Czech). */
function groupSeparator(locale: string): string {
  return (
    getFormatters(locale).number(1_000_000, { maximumFractionDigits: 0 }).replace(/\d/g, '')[0] ??
    ''
  );
}

/**
 * "3 500 000", "5,1", "5,400,000" or "1,234.5" → number; anything unreadable → 0.
 * Whitespace never matters. With both "," and "." the later one is the decimal
 * separator; a repeated separator groups thousands; a lone one is the decimal
 * separator unless it is the UI locale's group separator followed by exactly
 * three digits ("900,000" in English, while "4,125" stays 4.125 in Czech).
 */
export function parseCalcNumber(value: string, locale: string): number {
  let s = value.replace(/\s/g, '');
  const lastComma = s.lastIndexOf(',');
  const lastDot = s.lastIndexOf('.');
  if (lastComma >= 0 && lastDot >= 0) {
    const group = lastComma > lastDot ? '.' : ',';
    s = s.split(group).join('').replace(',', '.');
  } else if (lastComma >= 0 || lastDot >= 0) {
    const sep = lastComma >= 0 ? ',' : '.';
    const parts = s.split(sep);
    const grouped = parts.length > 2 || (sep === groupSeparator(locale) && parts[1].length === 3);
    s = parts.join(grouped ? '' : '.');
  }
  const n = parseFloat(s);
  return isFinite(n) ? n : 0;
}
