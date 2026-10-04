/** Typographic minus (U+2212): unambiguous next to currency symbols and in Czech text. */
export const MINUS_SIGN = '−';

/**
 * Formats a signed amount from a formatter of its absolute value, so a
 * deficit reads "−46 708 Kč" instead of a positive-looking number.
 *
 * The sign is dropped when the formatted magnitude shows no non-zero digit
 * (e.g. −0.3 formatted without decimals), so "−0 Kč" never appears.
 */
export function formatSignedAmount(value: number, formatAbs: (abs: number) => string): string {
  const formatted = formatAbs(Math.abs(value));
  if (value < 0 && /[1-9]/.test(formatted)) {
    return `${MINUS_SIGN}${formatted}`;
  }
  return formatted;
}
