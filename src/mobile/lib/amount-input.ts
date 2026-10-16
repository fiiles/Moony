/**
 * An amount typed on the phone's decimal keypad as the decimal text the backend stores
 * ("95 300,50" → "95300.50"), or `null` when the text is not a number. The keypad follows the
 * system language, so a comma is as likely as a point; spaces (also the no-break kinds of the
 * formatted placeholder) are thousands separators, and when both separators appear the last one is
 * the decimal one.
 */
export function parseAmountInput(text: string): string | null {
  let value = text.replace(/\s/g, '').replace(/−/g, '-');
  if (value === '') return null;

  const lastComma = value.lastIndexOf(',');
  const lastPoint = value.lastIndexOf('.');
  if (lastComma >= 0 && lastPoint >= 0) {
    const decimal = lastComma > lastPoint ? ',' : '.';
    const thousands = decimal === ',' ? '.' : ',';
    value = value.split(thousands).join('').replace(decimal, '.');
  } else {
    value = value.replace(',', '.');
  }

  if (!/^-?(\d+\.?\d*|\.\d+)$/.test(value)) return null;
  return value.replace(/\.$/, '').replace(/^(-?)\./, '$10.');
}

/**
 * `parseAmountInput` with the sign of the field's ± toggle: the iOS decimal pad has no minus key, so
 * an overdrawn balance is typed as its magnitude and made negative with the toggle. A minus that was
 * typed or pasted counts too (the toggle never flips it back); zero is written without a sign.
 */
export function signedAmountInput(text: string, negative: boolean): string | null {
  const parsed = parseAmountInput(text);
  if (parsed === null) return null;
  const magnitude = parsed.replace(/^-/, '');
  const isNegative = negative || parsed.startsWith('-');
  return isNegative && Number(magnitude) !== 0 ? `-${magnitude}` : magnitude;
}
