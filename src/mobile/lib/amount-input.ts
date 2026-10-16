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
