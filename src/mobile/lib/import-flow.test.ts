import { describe, expect, it } from 'vitest';
import type { CsvImportResult, CsvPreviewResult } from '@shared/schema';
import { maskMoney } from './hide-amounts';
import {
  chooseAccount,
  csvRecognizable,
  exactMoneyText,
  fileKind,
  importFacts,
  isOpenAccount,
  pickedFileStillWanted,
  presetReadable,
  previewAmount,
  recognitionAnswerWanted,
  statementNeedsStage,
} from './import-flow';
import { signedAmountText } from './signed-amount';

const DAY = 86_400;
/** UTC midnight of the "today" the tests run on (2026-10-19). */
const TODAY = Math.floor(Date.UTC(2026, 9, 19) / 1000);

const open = (id: string, institutionId: string | null = null) => ({
  id,
  institutionId,
  terminationDate: null,
});
const closed = (id: string, institutionId: string | null = null) => ({
  id,
  institutionId,
  terminationDate: TODAY - 3 * DAY,
});

describe('fileKind', () => {
  it('knows a PDF whatever the case of its extension', () => {
    expect(fileKind('vypis.PDF')).toBe('pdf');
    expect(fileKind('vypis.pdf')).toBe('pdf');
  });

  it('knows a photo or a scan', () => {
    expect(fileKind('scan.heic')).toBe('image');
    expect(fileKind('scan.HEIF')).toBe('image');
    expect(fileKind('IMG_1.jpg')).toBe('image');
    expect(fileKind('IMG_1.jpeg')).toBe('image');
    expect(fileKind('a.png')).toBe('image');
    expect(fileKind('a.gif')).toBe('image');
    expect(fileKind('a.webp')).toBe('image');
  });

  it('tries everything else as text, a file without an extension too', () => {
    expect(fileKind('export')).toBe('readable');
    expect(fileKind('x.sta')).toBe('readable');
    expect(fileKind('Fio.csv')).toBe('readable');
    expect(fileKind('statement.940')).toBe('readable');
    expect(fileKind('')).toBe('readable');
  });

  it('looks at the last extension only', () => {
    expect(fileKind('pdf.csv')).toBe('readable');
    expect(fileKind('statement.csv.pdf')).toBe('pdf');
  });
});

describe('isOpenAccount', () => {
  it('is open without a termination date, on its day and before it', () => {
    expect(isOpenAccount({ terminationDate: null }, TODAY)).toBe(true);
    expect(isOpenAccount({ terminationDate: TODAY }, TODAY)).toBe(true);
    expect(isOpenAccount({ terminationDate: TODAY + DAY }, TODAY)).toBe(true);
  });

  it('is closed after its termination date', () => {
    expect(isOpenAccount({ terminationDate: TODAY - DAY }, TODAY)).toBe(false);
  });
});

describe('chooseAccount', () => {
  const accounts = [open('a', 'inst_fio'), open('b', 'inst_revolut'), open('c', null)];

  it('takes the preselected account whatever else is known', () => {
    expect(
      chooseAccount({
        preselected: 'b',
        statement: { match: 'other', matchedAccountId: 'a' },
        presetInstitutionId: 'inst_fio',
        accounts,
        todayDay: TODAY,
      })
    ).toEqual({ accountId: 'b', reason: 'preselected' });
  });

  it('keeps a preselected closed account (its own detail page asked for it)', () => {
    expect(
      chooseAccount({ preselected: 'x', accounts: [closed('x'), open('a')], todayDay: TODAY })
    ).toEqual({ accountId: 'x', reason: 'preselected' });
  });

  it('takes the account a statement belongs to, selected or other', () => {
    expect(
      chooseAccount({
        statement: { match: 'selected', matchedAccountId: 'a' },
        accounts,
        todayDay: TODAY,
      })
    ).toEqual({ accountId: 'a', reason: 'statement' });
    expect(
      chooseAccount({
        statement: { match: 'other', matchedAccountId: 'c' },
        accounts,
        todayDay: TODAY,
      })
    ).toEqual({ accountId: 'c', reason: 'statement' });
  });

  it('does not take the account of a statement that names none, or a closed one', () => {
    expect(
      chooseAccount({
        statement: { match: 'none', matchedAccountId: null },
        accounts,
        todayDay: TODAY,
      }).reason
    ).toBe('ask');
    expect(
      chooseAccount({
        statement: { match: 'other', matchedAccountId: 'x' },
        accounts: [closed('x'), open('a'), open('b')],
        todayDay: TODAY,
      }).reason
    ).toBe('ask');
  });

  it('takes the one open account of the preset’s institution', () => {
    expect(
      chooseAccount({ presetInstitutionId: 'inst_revolut', accounts, todayDay: TODAY })
    ).toEqual({ accountId: 'b', reason: 'institution' });
  });

  it('never takes a closed account of the institution', () => {
    expect(
      chooseAccount({
        presetInstitutionId: 'inst_fio',
        accounts: [closed('old', 'inst_fio'), open('a', 'inst_fio'), open('b', 'inst_revolut')],
        todayDay: TODAY,
      })
    ).toEqual({ accountId: 'a', reason: 'institution' });
  });

  it('takes the only open account', () => {
    expect(chooseAccount({ accounts: [closed('x'), open('a')], todayDay: TODAY })).toEqual({
      accountId: 'a',
      reason: 'only',
    });
    expect(
      chooseAccount({
        presetInstitutionId: 'inst_unknown',
        accounts: [open('a', 'inst_fio')],
        todayDay: TODAY,
      })
    ).toEqual({ accountId: 'a', reason: 'only' });
  });

  it('asks when two accounts qualify', () => {
    expect(
      chooseAccount({
        presetInstitutionId: 'inst_fio',
        accounts: [open('a', 'inst_fio'), open('b', 'inst_fio'), open('c')],
        todayDay: TODAY,
      })
    ).toEqual({ accountId: null, reason: 'ask' });
    expect(chooseAccount({ accounts, todayDay: TODAY })).toEqual({
      accountId: null,
      reason: 'ask',
    });
  });

  it('asks when there is no open account at all', () => {
    expect(chooseAccount({ accounts: [closed('x')], todayDay: TODAY })).toEqual({
      accountId: null,
      reason: 'ask',
    });
    expect(chooseAccount({ accounts: [], todayDay: TODAY }).accountId).toBeNull();
  });
});

describe('importFacts', () => {
  const result = (patch: Partial<CsvImportResult>): CsvImportResult => ({
    importedCount: 0,
    duplicateCount: 0,
    errorCount: 0,
    errors: [],
    duplicates: [],
    skippedDuplicates: [],
    skippedZero: 0,
    skippedFiltered: 0,
    dateRange: null,
    uncategorizedCount: 0,
    lastBalance: null,
    suggestedRulePack: null,
    promotedCount: 0,
    statementBalance: null,
    ...patch,
  });
  const skipped = (line: number) => ({ line, key: 'csvImport.duplicate', detail: null });

  it('counts imported, skipped duplicates, errors and what the rules left open', () => {
    expect(
      importFacts(
        result({
          importedCount: 48,
          skippedDuplicates: [skipped(2), skipped(5), skipped(9)],
          errorCount: 1,
          uncategorizedCount: 12,
        })
      )
    ).toEqual({ imported: 48, duplicates: 3, errors: 1, uncategorized: 12, categorized: 36 });
  });

  it('never reports a negative number of categorized payments', () => {
    expect(importFacts(result({ importedCount: 2, uncategorizedCount: 5 })).categorized).toBe(0);
  });

  it('is all zero for an import that wrote nothing', () => {
    expect(importFacts(result({}))).toEqual({
      imported: 0,
      duplicates: 0,
      errors: 0,
      uncategorized: 0,
      categorized: 0,
    });
  });
});

describe('previewAmount', () => {
  it('reads the sign from the value and keeps the row’s own currency', () => {
    expect(previewAmount({ amount: '-250.5', currency: 'EUR' }, 'CZK')).toEqual({
      type: 'debit',
      amount: '-250.5',
      currency: 'EUR',
    });
    expect(previewAmount({ amount: '45000', currency: null }, 'CZK')).toEqual({
      type: 'credit',
      amount: '45000',
      currency: 'CZK',
    });
  });

  it('is null for an amount the file did not give or could not be read', () => {
    expect(previewAmount({ amount: null, currency: 'CZK' }, 'CZK')).toBeNull();
    expect(previewAmount({ amount: 'abc', currency: 'CZK' }, 'CZK')).toBeNull();
  });

  it('composes with signedAmountText, which keeps the sign while amounts are hidden', () => {
    const money = (value: number, currency: string) => `${value} ${currency}`;
    const masked = (value: number, currency: string) => maskMoney(money(value, currency));
    const debit = previewAmount({ amount: '-1249.5', currency: 'CZK' }, 'CZK');
    const credit = previewAmount({ amount: '800', currency: 'CZK' }, 'CZK');
    expect(signedAmountText(money, debit!)).toBe('− 1249.5 CZK');
    expect(signedAmountText(money, credit!)).toBe('+ 800 CZK');
    expect(signedAmountText(masked, debit!)).toBe('− •••• CZK');
    expect(signedAmountText(masked, credit!)).toBe('+ •••• CZK');
  });
});

describe('exactMoneyText', () => {
  const money = (value: number, currency: string, options?: { decimals?: number }) =>
    `${value.toFixed(options?.decimals ?? 0)} ${currency}`;

  it('shows the cents a balance has, at least two', () => {
    expect(exactMoneyText(money, '30196.12', 'CZK')).toBe('30196.12 CZK');
    expect(exactMoneyText(money, '250.5', 'EUR')).toBe('250.50 EUR');
  });

  it('shows none for a whole number', () => {
    expect(exactMoneyText(money, '1200', 'CZK')).toBe('1200 CZK');
  });

  it('shows at most eight decimals', () => {
    expect(exactMoneyText(money, '0.123456789', 'BTC')).toBe('0.12345679 BTC');
  });

  it('gives the text back for a value that is no number', () => {
    expect(exactMoneyText(money, 'n/a', 'CZK')).toBe('n/a');
  });
});

describe('statementNeedsStage', () => {
  const inspection = (statements: number, dateOrderAmbiguous = false) => ({
    statements: Array.from({ length: statements }, (_, index) => ({ index })),
    dateOrderAmbiguous,
  });

  it('is needed for a file with several statements or with dates that read both ways', () => {
    expect(statementNeedsStage(inspection(2))).toBe(true);
    expect(statementNeedsStage(inspection(1, true))).toBe(true);
  });

  it('is skipped for one statement with unambiguous dates', () => {
    expect(statementNeedsStage(inspection(1))).toBe(false);
  });
});

describe('CSV files', () => {
  const inspection = (patch: Partial<CsvPreviewResult>): CsvPreviewResult => ({
    headers: [],
    sampleRows: [],
    totalRows: 3,
    delimiter: ',',
    suggestedMappings: {},
    headerRow: 0,
    detectedPresetId: null,
    dateFormat: '%d.%m.%Y',
    dateFormatAmbiguous: false,
    decimalSeparator: ',',
    encoding: 'UTF-8',
    suggestedDescriptionColumns: [],
    suggestedRowFilter: null,
    ...patch,
  });

  describe('presetReadable', () => {
    it('goes straight to the preview with a detected preset that names a date and an amount', () => {
      expect(
        presetReadable(
          inspection({
            detectedPresetId: 'fio',
            suggestedMappings: { date: ['Datum', 1], amount: ['Objem', 1] },
          })
        )
      ).toBe(true);
    });

    it('accepts a debit and a credit column instead of one amount', () => {
      expect(
        presetReadable(
          inspection({
            detectedPresetId: 'uk-debit-credit',
            suggestedMappings: { date: ['Date', 1], debit: ['Out', 1], credit: ['In', 1] },
          })
        )
      ).toBe(true);
    });

    it('is not a preset file when no preset was detected, even if the headers name both columns', () => {
      // Rung 2 is a preset match only: any other CSV is recognized from its content
      expect(
        presetReadable(
          inspection({ suggestedMappings: { date: ['Date', 0.95], amount: ['Amount', 0.95] } })
        )
      ).toBe(false);
    });

    it('is not a preset file when the preset left the date or the amount out', () => {
      expect(
        presetReadable(
          inspection({ detectedPresetId: 'fio', suggestedMappings: { amount: ['Objem', 1] } })
        )
      ).toBe(false);
      expect(
        presetReadable(
          inspection({ detectedPresetId: 'fio', suggestedMappings: { date: ['Datum', 1] } })
        )
      ).toBe(false);
    });

    it('is not a preset file while the day and month of its dates are a guess', () => {
      expect(
        presetReadable(
          inspection({
            dateFormatAmbiguous: true,
            detectedPresetId: 'fio',
            suggestedMappings: { date: ['Datum', 1], amount: ['Objem', 1] },
          })
        )
      ).toBe(false);
    });
  });

  describe('csvRecognizable', () => {
    it('can be recognized with two named columns and a row to look at', () => {
      expect(csvRecognizable(inspection({ headers: ['A', 'B'], sampleRows: [['1', '2']] }))).toBe(
        true
      );
    });

    it('is no statement without rows, with one column or with no header', () => {
      expect(csvRecognizable(inspection({ headers: ['A', 'B'], sampleRows: [] }))).toBe(false);
      expect(csvRecognizable(inspection({ headers: ['A'], sampleRows: [['1']] }))).toBe(false);
      expect(csvRecognizable(inspection({ headers: ['', ' ', 'A'], sampleRows: [['1']] }))).toBe(
        false
      );
      expect(csvRecognizable(inspection({}))).toBe(false);
    });

    it('counts a header once however often the file repeats it', () => {
      expect(csvRecognizable(inspection({ headers: ['A', 'A'], sampleRows: [['1', '2']] }))).toBe(
        false
      );
    });
  });
});

describe('pickedFileStillWanted', () => {
  it('keeps a file the picker returned while the sheet that asked is still open', () => {
    expect(pickedFileStillWanted({ open: true, askedAt: 4, now: 4 })).toBe(true);
  });

  it('drops it when the sheet was closed while the picker was up (a lock, Android back)', () => {
    expect(pickedFileStillWanted({ open: false, askedAt: 4, now: 4 })).toBe(false);
  });

  it('drops it when another read or the close moved the generation on', () => {
    expect(pickedFileStillWanted({ open: true, askedAt: 4, now: 5 })).toBe(false);
  });
});

describe('recognitionAnswerWanted', () => {
  const tapped = { open: true, stage: 'recognize' as const, startedAt: 7, now: 7 };

  it('goes on when the sheet is still on the recognition rows and nothing came in between', () => {
    expect(recognitionAnswerWanted(tapped)).toBe(true);
  });

  it('is dropped when "Opravit…" was tapped meanwhile: it would bury the correction', () => {
    // Opening a question moves the stage on and bumps the generation: either alone is enough
    expect(recognitionAnswerWanted({ ...tapped, stage: 'correct' })).toBe(false);
    expect(recognitionAnswerWanted({ ...tapped, now: 8 })).toBe(false);
  });

  it('is dropped when the sheet was closed or the file replaced', () => {
    expect(recognitionAnswerWanted({ ...tapped, open: false })).toBe(false);
    expect(recognitionAnswerWanted({ ...tapped, stage: 'reading', now: 9 })).toBe(false);
  });
});
