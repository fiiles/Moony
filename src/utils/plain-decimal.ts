/**
 * Exact decimal text of a number without exponent notation (`String(1e-7)` is "1e-7").
 * No rounding: the shortest round-trip digits of the number are kept. Use it when a value
 * is sent to the backend as TEXT.
 */
export function toPlainDecimal(value: number): string {
  if (!Number.isFinite(value)) return '';
  const text = String(value);
  const match = /^(-?)(\d+)(?:\.(\d+))?e([+-]\d+)$/i.exec(text);
  if (!match) return text;
  const [, sign, int, frac = '', exp] = match;
  const digits = int + frac;
  const point = int.length + Number(exp);
  if (point <= 0) return `${sign}0.${'0'.repeat(-point)}${digits}`;
  if (point >= digits.length) return `${sign}${digits}${'0'.repeat(point - digits.length)}`;
  return `${sign}${digits.slice(0, point)}.${digits.slice(point)}`;
}

/**
 * Decimal text for an editable price input: up to 12 significant digits, never exponent
 * notation, trailing zeros trimmed.
 */
export function plainDecimal(value: number): string {
  if (!Number.isFinite(value)) return '';
  return toPlainDecimal(Number(value.toPrecision(12)));
}
