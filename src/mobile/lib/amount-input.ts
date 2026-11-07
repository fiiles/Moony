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

/** The decimal separator of `locale` ("," for cs, "." for en), from Intl rather than a table. */
export function decimalSeparator(locale: string): string {
  try {
    // eslint-disable-next-line no-restricted-syntax -- probes the separator only; nothing is displayed
    const part = new Intl.NumberFormat(locale).formatToParts(1.1).find((p) => p.type === 'decimal');
    return part?.value ?? '.';
  } catch {
    return '.';
  }
}

/**
 * A stored amount as the text an edit field starts with: no thousands grouping, the decimal
 * separator of `locale`, and no float noise (`0.1 + 0.2` reads "0,3"). Text that is not a plain
 * decimal (exponent form, empty) comes back unchanged, so a field never shows a wrong number.
 * `parseAmountInput` reads the result back.
 */
export function amountInputText(value: number | string, locale: string): string {
  const raw = typeof value === 'number' ? (Number.isFinite(value) ? String(value) : '') : value;
  let text = raw.trim();
  if (text === '') return '';
  if (typeof value === 'number') {
    // Round away binary noise (0.30000000000000004) before writing it out.
    text = String(Number(value.toPrecision(15)));
  }
  if (!/^-?\d*\.?\d+$|^-?\d+\.$/.test(text)) return raw;
  // Exponent form never matches above; a stored decimal text keeps its own digits ("12.50").
  return text.replace('.', decimalSeparator(locale));
}

/**
 * Typed decimal text as the dot-separated text the forms' schemas read (`Number("1.5")`): "1 234,5"
 * becomes "1234.5", and a trailing separator is dropped ("100," is "100"). A half-typed entry with
 * no digit yet ("-", ",") keeps its separator as a point so the field can go on; anything that is
 * not decimal text stays as it is (the schema then reports it).
 */
export function decimalTextToDot(text: string): string {
  const parsed = parseAmountInput(text);
  if (parsed !== null) return parsed;
  const compact = text.replace(/\s/g, '').replace(/−/g, '-');
  if (/^-?[.,]?$/.test(compact)) return compact.replace(',', '.');
  return text;
}

/**
 * What a decimal text field shows after its `value` prop changed: `null` keeps the text the person
 * typed, a string replaces it. The parent may hand the value back as it got it (dot text), through
 * its own transform (`parseFloat`, `Number`: a number, or `NaN` for an empty or half-typed entry),
 * or set something new from outside (a reset, a prefill). The text is kept while it reads as the
 * same number, so "1," survives the parent's "1", "0," its 0, and "0,3" its 0.30000000000000004;
 * `NaN` is the parent's echo of text that is not yet a number, so it is kept too. `undefined`, `null`
 * and "" are a clear from outside. A numeric 0 shows as an empty field (a form's untouched default).
 */
export function shownText(current: string, external: unknown, locale: string): string | null {
  if (external === undefined || external === null || external === '') {
    return current === '' ? null : '';
  }
  if (typeof external === 'number') {
    if (!Number.isFinite(external)) return null;
    if (external === 0 && current === '') return null;
    const typed = Number(decimalTextToDot(current));
    if (current.trim() !== '' && typed === Number(external.toPrecision(15))) return null;
    return external === 0 ? '' : amountInputText(external, locale);
  }
  if (typeof external !== 'string') return null;
  if (external === 'NaN' || external === 'Infinity' || external === '-Infinity') return null;
  const dot = decimalTextToDot(current);
  if (dot === external) return null;
  const typed = Number(dot);
  if (current.trim() !== '' && Number.isFinite(typed) && typed === Number(external)) return null;
  return amountInputText(external, locale);
}
