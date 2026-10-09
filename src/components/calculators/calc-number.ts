import { getFormatters } from '@/lib/format';

/** The locale's thousands separator ("," in English, a no-break space in Czech). */
function groupSeparator(locale: string): string {
  return (
    // No money is formatted here: no main currency needed.
    getFormatters(locale, undefined)
      .number(1_000_000, { maximumFractionDigits: 0 })
      .replace(/\d/g, '')[0] ?? ''
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

/** `value` rounded to `digits` significant digits (5 436 123 → 5 400 000). */
export function roundSignificant(value: number, digits = 2): number {
  if (value === 0 || !Number.isFinite(value)) return 0;
  const magnitude = 10 ** (Math.floor(Math.log10(Math.abs(value))) - digits + 1);
  return Math.round(value / magnitude) * magnitude;
}

/**
 * A calculator's starting amount, defined once in EUR and shown in the main
 * currency at the current quotes as a round number (140 000 EUR → "3 500 000"
 * in CZK). Empty when no rate is known: an amount is never carried over 1:1.
 */
export function scaledDefault(
  eur: number,
  convertFromEur: (amount: number, from: string) => number | null,
  format: (value: number) => string
): string {
  const converted = convertFromEur(eur, 'EUR');
  return converted === null ? '' : format(roundSignificant(converted));
}
