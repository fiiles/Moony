import { describe, expect, it } from 'vitest';
import {
  amountInputText,
  decimalTextToDot,
  shownText,
  parseAmountInput,
  signedAmountInput,
  setAmountInputLocale,
  groupAmountText,
  fieldUnitLabel,
} from './amount-input';

describe('parseAmountInput', () => {
  it('reads plain numbers', () => {
    expect(parseAmountInput('95300')).toBe('95300');
    expect(parseAmountInput('  1240.5 ')).toBe('1240.5');
    expect(parseAmountInput('0')).toBe('0');
  });

  it('takes a comma as the decimal separator', () => {
    expect(parseAmountInput('1240,50')).toBe('1240.50');
    expect(parseAmountInput(',5')).toBe('0.5');
  });

  it('drops spaces of every kind as thousands separators', () => {
    expect(parseAmountInput('95 300,50')).toBe('95300.50');
    expect(parseAmountInput('95 300')).toBe('95300');
    expect(parseAmountInput('1 234 567')).toBe('1234567');
  });

  it('lets the last separator be the decimal one when both appear', () => {
    expect(parseAmountInput('1.234,56')).toBe('1234.56');
    expect(parseAmountInput('1,234.56')).toBe('1234.56');
  });

  it('keeps a sign, also the typographic minus', () => {
    expect(parseAmountInput('-120,5')).toBe('-120.5');
    expect(parseAmountInput('−120')).toBe('-120');
  });

  it('drops a trailing separator', () => {
    expect(parseAmountInput('12,')).toBe('12');
  });

  it('is null for empty or non-numeric text', () => {
    expect(parseAmountInput('')).toBeNull();
    expect(parseAmountInput('   ')).toBeNull();
    expect(parseAmountInput('abc')).toBeNull();
    expect(parseAmountInput('12abc')).toBeNull();
    expect(parseAmountInput('1,2,3')).toBeNull();
    expect(parseAmountInput('1.2.3')).toBeNull();
    expect(parseAmountInput('-')).toBeNull();
    expect(parseAmountInput('+5')).toBeNull();
  });
});

describe('signedAmountInput', () => {
  it('is positive without the toggle', () => {
    expect(signedAmountInput('12 000', false)).toBe('12000');
    expect(signedAmountInput('1 240,50', false)).toBe('1240.50');
  });

  it('takes the sign from the toggle (the decimal pad has no minus key)', () => {
    expect(signedAmountInput('12 000', true)).toBe('-12000');
    expect(signedAmountInput('1 240,50', true)).toBe('-1240.50');
  });

  it('still accepts a typed or pasted minus', () => {
    expect(signedAmountInput('-12 000', false)).toBe('-12000');
    expect(signedAmountInput('\u221212 000', false)).toBe('-12000');
  });

  it('never flips a typed minus back with the toggle', () => {
    expect(signedAmountInput('-12 000', true)).toBe('-12000');
  });

  it('writes zero without a sign', () => {
    expect(signedAmountInput('0', true)).toBe('0');
    expect(signedAmountInput('0,00', true)).toBe('0.00');
    expect(signedAmountInput('-0', false)).toBe('0');
  });

  it('is null when the text is not a number, whatever the toggle says', () => {
    expect(signedAmountInput('', true)).toBeNull();
    expect(signedAmountInput('abc', true)).toBeNull();
    expect(signedAmountInput('12ab', false)).toBeNull();
  });
});

describe('amountInputText', () => {
  it('writes the locale decimal separator without grouping', () => {
    expect(amountInputText(95300.5, 'cs')).toBe('95300,5');
    expect(amountInputText('95300.50', 'cs')).toBe('95300,50');
    expect(amountInputText(95300.5, 'en')).toBe('95300.5');
    expect(amountInputText(-12.25, 'de')).toBe('-12,25');
  });

  it('removes float noise and keeps integers plain', () => {
    expect(amountInputText(0.1 + 0.2, 'cs')).toBe('0,3');
    expect(amountInputText(1000000, 'cs')).toBe('1000000');
    expect(amountInputText(0, 'en')).toBe('0');
  });

  it('leaves empty and non-decimal text alone', () => {
    expect(amountInputText('', 'cs')).toBe('');
    expect(amountInputText(Number.NaN, 'cs')).toBe('');
    expect(amountInputText('1e-7', 'cs')).toBe('1e-7');
  });

  it('round-trips through parseAmountInput', () => {
    for (const locale of ['cs', 'en', 'de', 'ja']) {
      expect(parseAmountInput(amountInputText(1234.56, locale))).toBe('1234.56');
    }
  });
});

describe('decimalTextToDot', () => {
  it('turns a comma or a grouped entry into the dot text', () => {
    expect(decimalTextToDot('1,5')).toBe('1.5');
    expect(decimalTextToDot('1 234,5')).toBe('1234.5');
    expect(decimalTextToDot('1.234,56')).toBe('1234.56');
    expect(decimalTextToDot('12.5')).toBe('12.5');
    expect(decimalTextToDot('')).toBe('');
  });

  it('keeps a half-typed entry going', () => {
    expect(decimalTextToDot('1,')).toBe('1');
    expect(decimalTextToDot('100,')).toBe('100');
    expect(decimalTextToDot('-')).toBe('-');
    expect(decimalTextToDot(',')).toBe('.');
  });

  it('leaves other text for the schema to reject', () => {
    expect(decimalTextToDot('abc')).toBe('abc');
  });
});

describe('shownText', () => {
  it('keeps the text when the parent echoes it as dot text', () => {
    expect(shownText('1,5', '1.5', 'cs')).toBeNull();
    expect(shownText('1,', '1', 'cs')).toBeNull();
    expect(shownText('-', '-', 'cs')).toBeNull();
    expect(shownText('0,', '0', 'cs')).toBeNull();
    expect(shownText('1,05', '1.05', 'cs')).toBeNull();
  });

  it('keeps the text through a parseFloat or Number parent', () => {
    expect(shownText('1,', 1, 'cs')).toBeNull();
    expect(shownText('0', 0, 'cs')).toBeNull();
    expect(shownText('0,', 0, 'cs')).toBeNull();
    expect(shownText('', 0, 'cs')).toBeNull();
    expect(shownText('1,05', 1.05, 'cs')).toBeNull();
    expect(shownText('0,3', 0.1 + 0.2, 'cs')).toBeNull();
  });

  it('keeps the text for NaN, the echo of an empty or half-typed entry', () => {
    expect(shownText('', Number.NaN, 'cs')).toBeNull();
    expect(shownText('-', Number.NaN, 'cs')).toBeNull();
    expect(shownText(',', Number.NaN, 'cs')).toBeNull();
  });

  it('clears on a reset to undefined, null or empty', () => {
    expect(shownText('5', undefined, 'cs')).toBe('');
    expect(shownText('5', null, 'cs')).toBe('');
    expect(shownText('5', '', 'cs')).toBe('');
    expect(shownText('', undefined, 'cs')).toBeNull();
  });

  it('shows a value set from outside, with the locale separator', () => {
    expect(shownText('5', 7.25, 'cs')).toBe('7,25');
    expect(shownText('', '12.50', 'cs')).toBe('12,50');
    expect(shownText('5', 0, 'cs')).toBe('');
    expect(shownText('', 0.1 + 0.2, 'cs')).toBe('0,3');
  });
});

describe('parseAmountInput with the UI locale (audit A7)', () => {
  it('reads a lone group separator before exactly three digits as thousands', () => {
    expect(parseAmountInput('1.234', 'de-DE')).toBe('1234');
    expect(parseAmountInput('1,234', 'en-US')).toBe('1234');
    expect(parseAmountInput('12.500', 'es-ES')).toBe('12500');
    expect(parseAmountInput('-1.234', 'de-DE')).toBe('-1234');
  });

  it('reads several groups of three', () => {
    expect(parseAmountInput('1.234.567', 'de-DE')).toBe('1234567');
    expect(parseAmountInput('1,234,567', 'ja-JP')).toBe('1234567');
  });

  it('keeps the separator decimal when it is not the locale group separator', () => {
    // Czech groups with a space: a point is a decimal one
    expect(parseAmountInput('1.234', 'cs-CZ')).toBe('1.234');
    expect(parseAmountInput('1,234', 'de-DE')).toBe('1.234');
    expect(parseAmountInput('1,234', 'cs-CZ')).toBe('1.234');
  });

  it('keeps the separator decimal when the digits after it are not exactly three', () => {
    expect(parseAmountInput('1.5', 'de-DE')).toBe('1.5');
    expect(parseAmountInput('1.2345', 'de-DE')).toBe('1.2345');
    expect(parseAmountInput('1,25', 'en-US')).toBe('1.25');
  });

  it('keeps a leading zero group decimal ("0.125" is not 125)', () => {
    expect(parseAmountInput('0.125', 'de-DE')).toBe('0.125');
    expect(parseAmountInput('.125', 'de-DE')).toBe('0.125');
    expect(parseAmountInput('1234.567', 'de-DE')).toBe('1234.567');
  });

  it('leaves text with both separators to the last-one-is-decimal rule', () => {
    expect(parseAmountInput('1.234,5', 'de-DE')).toBe('1234.5');
    expect(parseAmountInput('1,234.5', 'en-US')).toBe('1234.5');
  });

  it('behaves as before without a locale', () => {
    expect(parseAmountInput('1.234')).toBe('1.234');
    expect(parseAmountInput('1,234')).toBe('1.234');
  });

  it('takes the default locale set for the app', () => {
    setAmountInputLocale('de-DE');
    try {
      expect(parseAmountInput('1.234')).toBe('1234');
      expect(signedAmountInput('1.234', true)).toBe('-1234');
      expect(decimalTextToDot('2.500')).toBe('2500');
    } finally {
      setAmountInputLocale(undefined);
    }
  });
});

describe('groupAmountText (audit A5)', () => {
  it('groups the thousands of typed text in the UI locale and keeps the typed decimals', () => {
    expect(groupAmountText('12500', 'cs-CZ')).toBe('12 500');
    expect(groupAmountText('1234,5', 'cs-CZ')).toBe('1 234,5');
    expect(groupAmountText('12500', 'en-US')).toBe('12,500');
    expect(groupAmountText('1234.50', 'en-US')).toBe('1,234.50');
    expect(groupAmountText('12500', 'de-DE')).toBe('12.500');
    expect(groupAmountText('-1234567', 'de-DE')).toBe('-1.234.567');
  });

  it('follows the language rule for four digits (Polish and Spanish do not group them)', () => {
    expect(groupAmountText('1234', 'pl-PL')).toBe('1234');
    expect(groupAmountText('12345', 'pl-PL')).toBe('12 345');
    expect(groupAmountText('1234', 'es-ES')).toBe('1234');
  });

  it('reads back to the same amount', () => {
    for (const locale of ['cs-CZ', 'en-US', 'de-DE', 'ja-JP', 'pl-PL', 'sv-SE', 'nb-NO']) {
      const grouped = groupAmountText('1234567,25', locale);
      expect(parseAmountInput(grouped, locale)).toBe('1234567.25');
    }
  });

  it('leaves text that is not a number, half typed or empty as it is', () => {
    expect(groupAmountText('', 'cs-CZ')).toBe('');
    expect(groupAmountText('abc', 'cs-CZ')).toBe('abc');
    expect(groupAmountText('12,', 'cs-CZ')).toBe('12,');
    expect(groupAmountText('-', 'cs-CZ')).toBe('-');
    expect(groupAmountText('950', 'cs-CZ')).toBe('950');
  });
});

describe('fieldUnitLabel (audit A5)', () => {
  it('writes the main currency by its symbol, as rows do', () => {
    expect(fieldUnitLabel('CZK', 'CZK', 'cs-CZ')).toBe('Kč');
    expect(fieldUnitLabel('USD', 'USD', 'en-US')).toBe('$');
    expect(fieldUnitLabel('EUR', 'EUR', 'de-DE')).toBe('€');
  });

  it('writes any other currency by its code', () => {
    expect(fieldUnitLabel('EUR', 'CZK', 'cs-CZ')).toBe('EUR');
    expect(fieldUnitLabel('CZK', 'USD', 'en-US')).toBe('CZK');
  });

  it('leaves units that are not the main currency alone (a ticker, pieces, %)', () => {
    expect(fieldUnitLabel('BTC', 'CZK', 'cs-CZ')).toBe('BTC');
    expect(fieldUnitLabel('ks', 'CZK', 'cs-CZ')).toBe('ks');
    expect(fieldUnitLabel('%', 'CZK', 'cs-CZ')).toBe('%');
    expect(fieldUnitLabel('CZK', undefined, 'cs-CZ')).toBe('CZK');
  });
});
