/**
 * Decimal text for an editable price input: up to 12 significant digits, never exponent
 * notation (`String(1e-7)` is "1e-7"), trailing zeros trimmed.
 */
export function plainDecimal(value: number): string {
  if (!Number.isFinite(value)) return '';
  const rounded = Number(value.toPrecision(12));
  const text = String(rounded);
  if (!/e/i.test(text)) return text;
  return rounded
    .toFixed(20)
    .replace(/(\.\d*?)0+$/, '$1')
    .replace(/\.$/, '');
}
