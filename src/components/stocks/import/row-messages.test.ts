import { describe, expect, it } from 'vitest';
import type { TFunction } from 'i18next';
import type { StockRowMessage } from '@shared/schema';
import { ApiError } from '@/lib/api-error';
import {
  importErrorText,
  rowMessageLine,
  rowMessageText,
  type RowMessageContext,
} from './row-messages';

/** A translator over a small dictionary: `{{name}}` placeholders, `defaultValue` for unknown keys. */
function translator(dictionary: Record<string, string>): TFunction {
  const t = (key: string, options: Record<string, unknown> = {}) => {
    const template = dictionary[key];
    if (template === undefined) return (options.defaultValue as string | undefined) ?? key;
    return template.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(options[name] ?? ''));
  };
  return t as unknown as TFunction;
}

const context = (locale = 'en-US'): RowMessageContext => ({
  t: translator({
    'importWizard.row.notATrade': 'Not a purchase or a sale',
    'importWizard.row.dateUnparseable': 'The date could not be read',
    'importWizard.row.currencyInvalid': 'Invalid currency',
    'importWizard.row.tickerInvalid': 'Invalid symbol',
    'importWizard.row.symbolMissing': 'Symbol missing',
    'importWizard.row.symbolMissingIsin': 'Symbol missing, the ISIN "{{detail}}" is not valid',
    'importWizard.row.cannotParse': 'The row could not be read',
    'importWizard.row.sellExceedsHoldings': 'Sale exceeds the holding',
    'importWizard.row.sellExceedsHoldingsHeld': 'Sale exceeds the holding ({{detail}} held)',
    'importWizard.row.currencyMismatch': 'Currency differs from the position',
    'importWizard.row.currencyMismatchPosition': 'Currency differs from the position ({{detail}})',
    'importWizard.row.duplicate': 'The same trade is already in Moony',
    'importWizard.row.duplicateById': 'The same broker ID is already in Moony',
    'importWizard.row.writeFailed': 'The trade could not be written',
    'importWizard.result.line': 'Line {{line}}: {{message}}',
  }),
  tc: translator({ 'validation.somethingElse': 'Common sentence' }),
  locale,
});

const message = (key: string, detail: string | null = null, line = 8): StockRowMessage => ({
  line,
  key,
  detail,
});

describe('rowMessageText', () => {
  it('translates a key and puts the detail in parentheses', () => {
    expect(
      rowMessageText(message('importWizard.row.dateUnparseable', '31-02-2026'), context())
    ).toBe('The date could not be read (31-02-2026)');
    expect(rowMessageText(message('importWizard.row.notATrade', 'Deposit'), context())).toBe(
      'Not a purchase or a sale (Deposit)'
    );
  });

  it('leaves the parentheses out when there is no detail', () => {
    expect(rowMessageText(message('importWizard.row.notATrade'), context())).toBe(
      'Not a purchase or a sale'
    );
    expect(rowMessageText(message('importWizard.row.notATrade', '  '), context())).toBe(
      'Not a purchase or a sale'
    );
  });

  it('uses the sentence that works the detail in when there is one', () => {
    expect(rowMessageText(message('importWizard.row.currencyMismatch', 'USD'), context())).toBe(
      'Currency differs from the position (USD)'
    );
  });

  it('writes the held quantity in the notation of the UI language', () => {
    expect(
      rowMessageText(message('importWizard.row.sellExceedsHoldings', '2.5'), context('cs-CZ'))
    ).toBe('Sale exceeds the holding (2,5 held)');
    expect(rowMessageText(message('importWizard.row.sellExceedsHoldings', '2.5'), context())).toBe(
      'Sale exceeds the holding (2.5 held)'
    );
  });

  it('reads validation keys from the common namespace', () => {
    expect(rowMessageText(message('validation.somethingElse'), context())).toBe('Common sentence');
  });

  it('uses the row wording for an invalid currency', () => {
    expect(rowMessageText(message('validation.currencyInvalid', 'dollar'), context())).toBe(
      'Invalid currency (dollar)'
    );
  });

  it('uses the row wording for an invalid symbol', () => {
    expect(rowMessageText(message('validation.tickerInvalid', 'BRK B'), context())).toBe(
      'Invalid symbol (BRK B)'
    );
  });

  it('says what is wrong with the ISIN of a row that has no symbol, if it has one', () => {
    expect(rowMessageText(message('importWizard.row.symbolMissing'), context())).toBe(
      'Symbol missing'
    );
    expect(rowMessageText(message('importWizard.row.symbolMissing', 'XX12'), context())).toBe(
      'Symbol missing, the ISIN "XX12" is not valid'
    );
  });

  it('hides the technical detail of a record the reader could not read', () => {
    expect(
      rowMessageText(
        message('importWizard.row.cannotParse', 'CSV error: record 5 (line: 6, byte: 321)'),
        context()
      )
    ).toBe('The row could not be read');
  });

  it('words the two kinds of duplicate apart and hides their detail', () => {
    expect(rowMessageText(message('importWizard.row.duplicate'), context())).toBe(
      'The same trade is already in Moony'
    );
    expect(rowMessageText(message('importWizard.row.duplicateById', 'xtb:123'), context())).toBe(
      'The same broker ID is already in Moony'
    );
  });

  it('reads any other duplicate key as the plain duplicate', () => {
    expect(rowMessageText(message('importWizard.row.duplicateSameTrade', 'x'), context())).toBe(
      'The same trade is already in Moony'
    );
  });

  it('returns an unknown key as it is, so a missing translation stays visible', () => {
    expect(rowMessageText(message('importWizard.row.somethingNew'), context())).toBe(
      'importWizard.row.somethingNew'
    );
  });
});

describe('rowMessageLine', () => {
  it('puts the file line in front', () => {
    expect(rowMessageLine(message('importWizard.row.dateUnparseable', 'x', 12), context())).toBe(
      'Line 12: The date could not be read (x)'
    );
  });
});

describe('importErrorText', () => {
  const normalized = (raw: string, message: string) =>
    new ApiError({ kind: 'validation', message, raw }, undefined);

  it('translates the row message a failed write names', () => {
    const error = normalized(
      'Validation error: importWizard.row.sellExceedsHoldings',
      'importWizard.row.sellExceedsHoldings'
    );
    expect(importErrorText(error, context())).toBe('Sale exceeds the holding');
  });

  it('finds the key in a plain error too', () => {
    expect(importErrorText(new Error('importWizard.row.notATrade'), context())).toBe(
      'Not a purchase or a sale'
    );
  });

  it('keeps the message of an error that is already translated', () => {
    const error = normalized('Validation error: validation.somethingElse', 'Already translated');
    expect(importErrorText(error, context())).toBe('Already translated');
  });

  it('names the row of a write that found a rule broken', () => {
    const error = normalized(
      'Validation error: line 12: importWizard.row.sellExceedsHoldings',
      'line 12: importWizard.row.sellExceedsHoldings'
    );
    expect(importErrorText(error, context())).toBe('Line 12: Sale exceeds the holding');
  });

  it('translates the validation key a write failed with', () => {
    const error = normalized(
      'Validation error: line 7: validation.somethingElse',
      'line 7: validation.somethingElse'
    );
    expect(importErrorText(error, context())).toBe('Line 7: Common sentence');
  });

  it('does not show a reason that is not a key, only the row', () => {
    const error = normalized(
      'Validation error: line 7: Database error: UNIQUE constraint failed: x.y',
      'line 7: Database error: UNIQUE constraint failed: x.y'
    );
    expect(importErrorText(error, context())).toBe('Line 7: The trade could not be written');
  });

  it('translates a bare validation key of a plain error', () => {
    expect(importErrorText(new Error('validation.somethingElse'), context())).toBe(
      'Common sentence'
    );
    expect(importErrorText('boom', context())).toBe('boom');
  });
});
