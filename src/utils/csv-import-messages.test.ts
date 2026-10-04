import { describe, expect, it } from 'vitest';
import type { TFunction } from 'i18next';
import type { CsvRowMessage } from '@shared/schema';
import { formatRowMessage, translateRowMessage } from './csv-import-messages';

type Dictionary = Record<string, string>;

/** Minimal i18next stand-in: `{{name}}` interpolation plus `defaultValue`. */
function fakeT(dictionary: Dictionary): TFunction {
  return ((key: string, options?: Record<string, unknown>) => {
    const template = dictionary[key] ?? (options?.defaultValue as string | undefined) ?? key;
    return template.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(options?.[name] ?? ''));
  }) as unknown as TFunction;
}

const t = fakeT({
  'csvImport.rowMessage': 'Row {{line}}: {{message}}',
  'csvImport.rowMessageDetail': 'Row {{line}}: {{message}} ({{detail}})',
  'csvImport.dateUnparseable': 'The date could not be read',
  'csvImport.zeroAmount': 'Zero amount, skipped',
  'csvImport.duplicate': 'An identical transaction already exists',
  // A bank_accounts key that happens to exist must never answer for validation.*
  'validation.amountRequired': 'WRONG NAMESPACE',
});
const tc = fakeT({
  'validation.amountRequired': 'Amount is required',
  'validation.currencyInvalid': 'Currency must be 3 letters (e.g., USD, EUR)',
});

const message = (overrides: Partial<CsvRowMessage>): CsvRowMessage => ({
  line: 5,
  key: 'csvImport.zeroAmount',
  detail: null,
  ...overrides,
});

describe('translateRowMessage', () => {
  it('translates csvImport.* keys from the bank_accounts namespace', () => {
    expect(translateRowMessage(message({ key: 'csvImport.duplicate' }), t, tc)).toBe(
      'An identical transaction already exists'
    );
  });

  it('translates validation.* keys from the common namespace', () => {
    expect(translateRowMessage(message({ key: 'validation.amountRequired' }), t, tc)).toBe(
      'Amount is required'
    );
    expect(translateRowMessage(message({ key: 'validation.currencyInvalid' }), t, tc)).toBe(
      'Currency must be 3 letters (e.g., USD, EUR)'
    );
  });

  it('leaves an unknown key visible instead of rendering nothing', () => {
    expect(translateRowMessage(message({ key: 'csvImport.somethingNew' }), t, tc)).toBe(
      'csvImport.somethingNew'
    );
    expect(translateRowMessage(message({ key: 'validation.somethingNew' }), t, tc)).toBe(
      'validation.somethingNew'
    );
  });
});

describe('formatRowMessage', () => {
  it('prefixes the file line', () => {
    expect(formatRowMessage(message({ line: 12, key: 'csvImport.zeroAmount' }), t, tc)).toBe(
      'Row 12: Zero amount, skipped'
    );
  });

  it('appends the raw detail in parentheses', () => {
    expect(
      formatRowMessage(
        message({ line: 8, key: 'csvImport.dateUnparseable', detail: '31-02-2026' }),
        t,
        tc
      )
    ).toBe('Row 8: The date could not be read (31-02-2026)');
  });

  it('formats a translated validation key like any other message', () => {
    expect(formatRowMessage(message({ line: 5, key: 'validation.amountRequired' }), t, tc)).toBe(
      'Row 5: Amount is required'
    );
  });

  it('ignores a blank or missing detail', () => {
    expect(formatRowMessage(message({ detail: '   ' }), t, tc)).toBe('Row 5: Zero amount, skipped');
    expect(formatRowMessage(message({ detail: null }), t, tc)).toBe('Row 5: Zero amount, skipped');
  });
});
