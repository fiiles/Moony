/**
 * The UI locale the phone's amount fields read by default (`setAmountInputLocale`, set by
 * `MobileApp` from the profile language). The desktop never sets it, so its fields keep the
 * locale-free reading.
 */
let defaultLocale: string | undefined;

/** Sets the locale `parseAmountInput` reads with when a caller names none; `undefined` clears it. */
export function setAmountInputLocale(locale: string | undefined): void {
  defaultLocale = locale;
}

/** The thousands separator of `locale` ("." for de, "," for en, a no-break space for cs). */
function groupSeparator(locale: string): string {
  try {
    // eslint-disable-next-line no-restricted-syntax -- probes the separator only; nothing is displayed
    const part = new Intl.NumberFormat(locale).formatToParts(11111).find((p) => p.type === 'group');
    return part?.value ?? '';
  } catch {
    return '';
  }
}

/**
 * An amount typed on the phone's decimal keypad as the decimal text the backend stores
 * ("95 300,50" → "95300.50"), or `null` when the text is not a number. The keypad follows the
 * system language, so a comma is as likely as a point; spaces (also the no-break kinds of the
 * formatted placeholder) are thousands separators, and when both separators appear the last one is
 * the decimal one. A lone kind of separator is decimal, except where it is the group separator of
 * `locale` (by default the app's, `setAmountInputLocale`) and every group after it has exactly three
 * digits behind a first group of one to three that does not start with 0: a German "1.234" and an
 * English "1,234" are 1234, a German "1,5" and a Czech "1.234" stay decimal (audit A7).
 */
export function parseAmountInput(
  text: string,
  locale: string | undefined = defaultLocale
): string | null {
  let value = text.replace(/\s/g, '').replace(/−/g, '-');
  if (value === '') return null;

  const lastComma = value.lastIndexOf(',');
  const lastPoint = value.lastIndexOf('.');
  if (lastComma >= 0 && lastPoint >= 0) {
    const decimal = lastComma > lastPoint ? ',' : '.';
    const thousands = decimal === ',' ? '.' : ',';
    value = value.split(thousands).join('').replace(decimal, '.');
  } else {
    const separator = lastComma >= 0 ? ',' : lastPoint >= 0 ? '.' : null;
    const groups = separator === null ? [] : value.replace(/^-/, '').split(separator);
    const grouped =
      separator !== null &&
      locale !== undefined &&
      groupSeparator(locale) === separator &&
      /^[1-9]\d{0,2}$/.test(groups[0]) &&
      groups.slice(1).every((group) => /^\d{3}$/.test(group));
    value = grouped ? value.split(separator).join('') : value.replace(',', '.');
  }

  if (!/^-?(\d+\.?\d*|\.\d+)$/.test(value)) return null;
  return value.replace(/\.$/, '').replace(/^(-?)\./, '$10.');
}

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

/**
 * Typed amount text with its thousands grouped the way `locale` writes them, for a field that has
 * lost focus ("12500" → "12 500", "1234,5" → "1 234,5"; Polish and Spanish leave four digits
 * alone). The typed decimals stay as they are; text that is not a whole number yet (empty, "12,",
 * "-", letters) comes back unchanged. `parseAmountInput` reads the result back (audit A5).
 */
export function groupAmountText(text: string, locale: string): string {
  if (/[.,]$/.test(text.trim())) return text;
  const parsed = parseAmountInput(text, locale);
  if (parsed === null) return text;
  const negative = parsed.startsWith('-');
  const [whole, fraction] = parsed.replace(/^-/, '').split('.');
  let grouped: string;
  try {
    // eslint-disable-next-line no-restricted-syntax -- groups typed digits; BigInt keeps every digit
    grouped = new Intl.NumberFormat(locale).format(BigInt(whole));
  } catch {
    return text;
  }
  const decimals = fraction === undefined ? '' : `${decimalSeparator(locale)}${fraction}`;
  return `${negative ? '-' : ''}${grouped}${decimals}`;
}

/**
 * The unit an amount field shows: the main currency by its symbol ("Kč", "$", "€"), as rows write
 * it, anything else as given (another currency by its code, a ticker, "ks", "%"). Audit A5.
 */
export function fieldUnitLabel(
  unit: string,
  mainCurrency: string | undefined,
  locale: string
): string {
  const code = unit.trim().toUpperCase();
  if (!mainCurrency || !/^[A-Z]{3}$/.test(code) || code !== mainCurrency.trim().toUpperCase()) {
    return unit;
  }
  try {
    // eslint-disable-next-line no-restricted-syntax -- reads the symbol only; nothing is formatted
    const part = new Intl.NumberFormat(locale, {
      style: 'currency',
      currency: code,
      currencyDisplay: 'narrowSymbol',
    })
      .formatToParts(1)
      .find((p) => p.type === 'currency');
    return part?.value ?? unit;
  } catch {
    return unit;
  }
}
