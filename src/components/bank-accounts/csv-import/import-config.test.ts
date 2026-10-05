import { describe, expect, it } from 'vitest';
import type {
  BankAccount,
  CsvImportPreview,
  CsvImportResult,
  CsvPreset,
  CsvPreviewResult,
  CsvPreviewRow,
} from '@shared/schema';
import {
  AUTO_VALUE,
  EMPTY_MAPPING,
  SAMPLE_CSV,
  balanceDiffers,
  balanceUpdatePayload,
  buildImportConfig,
  dateFormatChoices,
  dateFormatLabel,
  fileNameFromPath,
  formatBalance,
  formatPreviewAmount,
  isDateFormatGuess,
  isMappingComplete,
  isSupportedCsvPath,
  mappingFromInspection,
  nextStepsFor,
  pickDroppedFile,
  presetIdForInstitution,
  presetLabel,
  regionName,
  rowStatusPresentation,
  sortPresets,
  suggestionConfidence,
  uniqueHeaders,
  summarizePreview,
  usesDebitCredit,
  type MappingState,
  type NextStepContext,
} from './import-config';

const inspection = (overrides: Partial<CsvPreviewResult> = {}): CsvPreviewResult => ({
  headers: ['Date', 'Description', 'Amount', 'Currency'],
  sampleRows: [],
  totalRows: 3,
  delimiter: ',',
  suggestedMappings: {
    date: ['Date', 1],
    amount: ['Amount', 0.95],
    description: ['Description', 0.8],
    currency: ['Currency', 0.5],
  },
  headerRow: 0,
  detectedPresetId: null,
  dateFormat: '%Y-%m-%d',
  dateFormatAmbiguous: false,
  decimalSeparator: '.',
  encoding: 'UTF-8',
  suggestedDescriptionColumns: ['Description'],
  suggestedRowFilter: null,
  ...overrides,
});

const mapping = (overrides: Partial<MappingState> = {}): MappingState => ({
  ...EMPTY_MAPPING,
  dateColumn: 'Date',
  amountColumn: 'Amount',
  ...overrides,
});

describe('mappingFromInspection', () => {
  it('fills the columns from the suggestions and the format from the detection', () => {
    const m = mappingFromInspection(
      inspection({ dateFormat: '%d.%m.%Y', suggestedRowFilter: null })
    );
    expect(m).toMatchObject({
      dateColumn: 'Date',
      amountColumn: 'Amount',
      descriptionColumns: ['Description'],
      currencyColumn: 'Currency',
      debitColumn: '',
      dateFormat: '%d.%m.%Y',
      decimalSeparator: AUTO_VALUE,
      rowFilterEnabled: false,
    });
  });

  it('maps debit/credit, balance, transaction id and the legacy counterparty_iban role', () => {
    const m = mappingFromInspection(
      inspection({
        suggestedMappings: {
          date: ['Booking date', 1],
          debit: ['Paid out', 1],
          credit: ['Paid in', 1],
          balance: ['Balance', 1],
          transactionId: ['ID', 1],
          counterparty: ['Name', 1],
          counterparty_iban: ['IBAN', 1],
        },
      })
    );
    expect(m).toMatchObject({
      dateColumn: 'Booking date',
      amountColumn: '',
      debitColumn: 'Paid out',
      creditColumn: 'Paid in',
      balanceColumn: 'Balance',
      transactionIdColumn: 'ID',
      counterpartyColumn: 'Name',
      counterpartyIbanColumn: 'IBAN',
    });
  });

  it('turns the row filter on when the preset suggests one', () => {
    const m = mappingFromInspection(
      inspection({ suggestedRowFilter: { column: 'State', equals: 'COMPLETED' } })
    );
    expect(m.rowFilterEnabled).toBe(true);
  });

  it('falls back to Auto when no date format was detected', () => {
    expect(mappingFromInspection(inspection({ dateFormat: '' })).dateFormat).toBe(AUTO_VALUE);
  });
});

describe('uniqueHeaders', () => {
  it('drops blank cells and repeated names so every option has a unique value', () => {
    expect(uniqueHeaders(['Date', '', ' ', 'Amount', 'Date', 'Note'])).toEqual([
      'Date',
      'Amount',
      'Note',
    ]);
  });
});

describe('isMappingComplete / usesDebitCredit', () => {
  it('needs a date column', () => {
    expect(isMappingComplete(mapping({ dateColumn: '' }))).toBe(false);
  });

  it('accepts an amount column or a full debit/credit pair, not half a pair', () => {
    expect(isMappingComplete(mapping())).toBe(true);
    expect(
      isMappingComplete(mapping({ amountColumn: '', debitColumn: 'Debit', creditColumn: 'Credit' }))
    ).toBe(true);
    expect(isMappingComplete(mapping({ amountColumn: '', debitColumn: 'Debit' }))).toBe(false);
    expect(isMappingComplete(mapping({ amountColumn: '' }))).toBe(false);
  });

  it('shows the credit − debit hint only when no amount column is chosen', () => {
    const pair = { debitColumn: 'Debit', creditColumn: 'Credit' };
    expect(usesDebitCredit(mapping({ amountColumn: '', ...pair }))).toBe(true);
    expect(usesDebitCredit(mapping({ ...pair }))).toBe(false);
  });
});

describe('suggestionConfidence', () => {
  const ins = inspection();

  it('reports high and medium confidence for the suggested column', () => {
    const m = mappingFromInspection(ins);
    expect(suggestionConfidence(ins, m, 'date')).toEqual({ level: 'high', percent: 100 });
    expect(suggestionConfidence(ins, m, 'amount')).toEqual({ level: 'high', percent: 95 });
    expect(suggestionConfidence(ins, m, 'description')).toEqual({ level: 'medium', percent: 80 });
  });

  it('shows nothing below 0.7, without a suggestion, or after the user chose another column', () => {
    const m = mappingFromInspection(ins);
    expect(suggestionConfidence(ins, m, 'currency')).toBeNull();
    expect(suggestionConfidence(ins, m, 'balance')).toBeNull();
    expect(suggestionConfidence(ins, { ...m, dateColumn: 'Description' }, 'date')).toBeNull();
    expect(suggestionConfidence(ins, { ...m, descriptionColumns: [] }, 'description')).toBeNull();
  });
});

describe('buildImportConfig', () => {
  it('carries the file facts, mapping and the strict defaults', () => {
    const config = buildImportConfig(
      mapping({
        dateFormat: '%d.%m.%Y',
        decimalSeparator: ',',
        descriptionColumns: ['Description'],
      }),
      { delimiter: ';', headerRow: 7, encoding: 'windows-1250', suggestedRowFilter: null },
      2
    );
    expect(config).toMatchObject({
      delimiter: ';',
      headerRow: 7,
      skipRows: 2,
      encoding: 'windows-1250',
      dateColumn: 'Date',
      dateFormat: '%d.%m.%Y',
      amountColumn: 'Amount',
      decimalSeparator: ',',
      descriptionColumns: ['Description'],
      skipDuplicates: true,
      importAnywayLines: [],
      rowFilter: null,
    });
  });

  it('sends empty optional columns as null, not as empty strings', () => {
    const config = buildImportConfig(mapping(), inspection(), 0);
    expect(config.counterpartyColumn).toBeNull();
    expect(config.counterpartyIbanColumn).toBeNull();
    expect(config.currencyColumn).toBeNull();
    expect(config.balanceColumn).toBeNull();
    expect(config.transactionIdColumn).toBeNull();
    expect(config.descriptionColumns).toBeNull();
  });

  it('sends a debit/credit pair only when both columns are set', () => {
    const pair = buildImportConfig(
      mapping({ amountColumn: '', debitColumn: 'Debit', creditColumn: 'Credit' }),
      inspection(),
      0
    );
    expect(pair).toMatchObject({
      amountColumn: null,
      debitColumn: 'Debit',
      creditColumn: 'Credit',
    });

    const half = buildImportConfig(mapping({ debitColumn: 'Debit' }), inspection(), 0);
    expect(half).toMatchObject({ amountColumn: 'Amount', debitColumn: null, creditColumn: null });
  });

  it('applies the preset row filter only while the checkbox is on', () => {
    const filter = { column: 'State', equals: 'COMPLETED' };
    const ins = inspection({ suggestedRowFilter: filter });
    expect(buildImportConfig(mapping({ rowFilterEnabled: true }), ins, 0).rowFilter).toEqual(
      filter
    );
    expect(buildImportConfig(mapping({ rowFilterEnabled: false }), ins, 0).rowFilter).toBeNull();
  });

  it('sorts the import-anyway lines so the same choice always gives the same config', () => {
    const config = buildImportConfig(mapping(), inspection(), 0, [12, 3, 7]);
    expect(config.importAnywayLines).toEqual([3, 7, 12]);
  });

  it('falls back to Auto for an empty date format', () => {
    expect(buildImportConfig(mapping({ dateFormat: '' }), inspection(), 0).dateFormat).toBe('auto');
  });
});

describe('date format helpers', () => {
  it('offers Auto and the known formats, plus any other format that is in use', () => {
    expect(dateFormatChoices('%d.%m.%Y')).toEqual([
      'auto',
      '%Y-%m-%d',
      '%d.%m.%Y',
      '%d/%m/%Y',
      '%m/%d/%Y',
      '%d-%m-%Y',
      '%d.%m.%y',
      '%d-%m-%y',
      '%d/%m/%y',
      '%m/%d/%y',
      '%y-%m-%d',
    ]);
    const custom = dateFormatChoices('%Y/%m/%d', '%Y/%m/%d', null, '');
    expect(custom.at(-1)).toBe('%Y/%m/%d');
    expect(custom.filter((v) => v === '%Y/%m/%d')).toHaveLength(1);
  });

  it('labels known formats with notation and unknown ones with their pattern', () => {
    expect(dateFormatLabel('%d.%m.%Y')).toBe('DD.MM.YYYY');
    expect(dateFormatLabel('%Y/%m/%d')).toBe('%Y/%m/%d');
  });

  it('flags the day/month order as a guess until the user picks another format', () => {
    const ambiguous = { dateFormat: '%d/%m/%Y', dateFormatAmbiguous: true };
    expect(isDateFormatGuess(ambiguous, '%d/%m/%Y')).toBe(true);
    expect(isDateFormatGuess(ambiguous, 'auto')).toBe(true);
    expect(isDateFormatGuess(ambiguous, '%m/%d/%Y')).toBe(false);
    expect(isDateFormatGuess({ ...ambiguous, dateFormatAmbiguous: false }, '%d/%m/%Y')).toBe(false);
  });
});

describe('summarizePreview', () => {
  const row = (line: number, status: CsvPreviewRow['status']): CsvPreviewRow => ({
    line,
    bookingDate: 1_700_000_000,
    amount: '-10',
    currency: 'CZK',
    description: null,
    counterparty: null,
    status,
    message: null,
    categoryId: null,
    categorySource: null,
  });
  const preview = (overrides: Partial<CsvImportPreview> = {}): CsvImportPreview => ({
    rows: [
      row(2, 'ok'),
      row(3, 'duplicate'),
      row(4, 'duplicate'),
      row(5, 'error'),
      row(6, 'skipped'),
    ],
    totalRows: 5,
    okCount: 1,
    errorCount: 1,
    duplicateCount: 2,
    skippedCount: 1,
    dateRange: null,
    categorizedCount: 0,
    ...overrides,
  });

  it('splits the rows to write into categorized and uncategorized', () => {
    expect(summarizePreview(preview({ okCount: 5, categorizedCount: 3 }), [])).toMatchObject({
      willImport: 5,
      categorized: 3,
      uncategorized: 2,
    });
    // A forced duplicate counts in both totals the backend already includes it in.
    expect(summarizePreview(preview({ categorizedCount: 2 }), [3])).toMatchObject({
      willImport: 2,
      categorized: 2,
      uncategorized: 0,
    });
  });

  it('counts duplicates as skipped by default', () => {
    expect(summarizePreview(preview(), [])).toMatchObject({
      willImport: 1,
      forcedDuplicates: 0,
      skippedDuplicates: 2,
      errors: 1,
      skipped: 1,
    });
  });

  it('adds a ticked duplicate to the rows that will be imported', () => {
    expect(summarizePreview(preview(), [4])).toMatchObject({
      willImport: 2,
      forcedDuplicates: 1,
      skippedDuplicates: 1,
    });
  });

  it('ignores ticked lines that are no longer duplicates', () => {
    expect(summarizePreview(preview(), [2, 5, 99])).toMatchObject({
      willImport: 1,
      forcedDuplicates: 0,
      skippedDuplicates: 2,
    });
  });

  it('keeps the totals right when the listing is capped', () => {
    expect(
      summarizePreview(preview({ okCount: 500, duplicateCount: 40, totalRows: 560 }), [3])
    ).toMatchObject({ willImport: 501, skippedDuplicates: 39 });
  });
});

describe('formatPreviewAmount', () => {
  it('signs credits with a plus and debits with a real minus', () => {
    expect(formatPreviewAmount('45000', 'CZK', 'en-US')).toBe('+K\u010d\u00a045,000');
    expect(formatPreviewAmount('-1234.5', 'USD', 'en-US')).toBe('\u2212$1,234.50');
  });

  it('shows a dash when the row has no amount and the raw text when it is not a number', () => {
    expect(formatPreviewAmount(null, 'CZK', 'en-US')).toBe('—');
    expect(formatPreviewAmount('abc', 'CZK', 'en-US')).toBe('abc');
  });

  it('never rounds away cents, not even for CZK (which the app shows without decimals)', () => {
    expect(formatPreviewAmount('-250.5', 'CZK', 'en-US')).toBe('\u2212K\u010d\u00a0250.50');
    expect(formatPreviewAmount('0.05', 'CZK', 'en-US')).toBe('+K\u010d\u00a00.05');
    expect(formatPreviewAmount('12.345', 'USD', 'en-US')).toBe('+$12.345');
  });

  it('formats without a currency code when the row has none', () => {
    expect(formatPreviewAmount('12.5', null, 'en-US')).toBe('+12.50');
  });
});

describe('rowStatusPresentation', () => {
  it('gives every status its own badge', () => {
    expect(rowStatusPresentation('ok')).toEqual({
      labelKey: 'csvImport.status.ok',
      variant: 'gain',
    });
    expect(rowStatusPresentation('error').variant).toBe('loss');
    expect(rowStatusPresentation('duplicate').variant).toBe('default');
    expect(rowStatusPresentation('skipped').variant).toBe('outline');
  });
});

describe('file helpers', () => {
  it('accepts .csv and .txt files, any case', () => {
    expect(isSupportedCsvPath('/Users/me/statement.csv')).toBe(true);
    expect(isSupportedCsvPath('C:\\Users\\me\\STATEMENT.CSV')).toBe(true);
    expect(isSupportedCsvPath('/tmp/export.txt')).toBe(true);
    expect(isSupportedCsvPath('/tmp/statement.pdf')).toBe(false);
    expect(isSupportedCsvPath('/tmp/csv')).toBe(false);
    expect(isSupportedCsvPath('/tmp/folder.csv.bak')).toBe(false);
  });

  it('picks the first importable file of a drop', () => {
    expect(pickDroppedFile(['/a/photo.png', '/a/b.csv', '/a/c.csv'])).toBe('/a/b.csv');
    expect(pickDroppedFile(['/a/photo.png'])).toBeNull();
    expect(pickDroppedFile([])).toBeNull();
  });

  it('takes the file name from both kinds of path', () => {
    expect(fileNameFromPath('/Users/me/statement.csv')).toBe('statement.csv');
    expect(fileNameFromPath('C:\\Users\\me\\statement.csv')).toBe('statement.csv');
    expect(fileNameFromPath('statement.csv')).toBe('statement.csv');
  });

  it('ships a sample the importer can read: header plus three rows', () => {
    const lines = SAMPLE_CSV.trim().split('\n');
    expect(lines[0]).toBe('Date,Description,Amount,Currency,Counterparty');
    expect(lines).toHaveLength(4);
    expect(lines.slice(1).every((line) => line.split(',').length === 5)).toBe(true);
  });
});

describe('presets', () => {
  const preset = (overrides: Partial<CsvPreset>): CsvPreset =>
    ({
      id: 'x',
      bankName: 'X',
      country: 'CZ',
      institutionId: null,
      ...overrides,
    }) as CsvPreset;

  it('sorts by bank name ignoring diacritics and labels with the country', () => {
    const sorted = sortPresets([
      preset({ id: 'wise', bankName: 'Wise', country: 'BE' }),
      preset({ id: 'cs', bankName: 'Česká spořitelna' }),
      preset({ id: 'air', bankName: 'Air Bank' }),
      preset({ id: 'csob', bankName: 'ČSOB' }),
    ]);
    expect(sorted.map((p) => p.id)).toEqual(['air', 'cs', 'csob', 'wise']);
    expect(presetLabel(sorted[3])).toBe('Wise (BE)');
  });

  it('finds the preset seeded for an institution', () => {
    const presets = [
      preset({ id: 'revolut', institutionId: 'inst_revolut' }),
      preset({ id: 'fio' }),
    ];
    expect(presetIdForInstitution(presets, 'inst_revolut')).toBe('revolut');
    expect(presetIdForInstitution(presets, 'inst_other')).toBeNull();
    expect(presetIdForInstitution(presets, null)).toBeNull();
    expect(presetIdForInstitution(presets, undefined)).toBeNull();
  });
});

describe('next steps', () => {
  const result = (overrides: Partial<CsvImportResult> = {}): CsvImportResult => ({
    importedCount: 10,
    duplicateCount: 0,
    errorCount: 0,
    errors: [],
    duplicates: [],
    skippedDuplicates: [],
    skippedZero: 0,
    skippedFiltered: 0,
    dateRange: { from: 1_757_000_000, to: 1_759_000_000 },
    uncategorizedCount: 7,
    lastBalance: '30196.12',
    suggestedRulePack: 'de',
    ...overrides,
  });
  const context = (overrides: Partial<NextStepContext> = {}): NextStepContext => ({
    canShowImported: true,
    canReviewUncategorized: true,
    accountBalance: '1000',
    ...overrides,
  });

  it('offers everything that applies, in the documented order', () => {
    expect(nextStepsFor(result(), context())).toEqual(['show', 'review', 'rulePack', 'balance']);
  });

  it('offers nothing when nothing applies', () => {
    expect(
      nextStepsFor(
        result({ uncategorizedCount: 0, suggestedRulePack: null, lastBalance: null }),
        context({ canShowImported: false })
      )
    ).toEqual([]);
  });

  it('does not offer to show transactions when none were imported', () => {
    expect(nextStepsFor(result({ importedCount: 0 }), context())).not.toContain('show');
  });

  it('needs a caller that can act on "show" and "review"', () => {
    const steps = nextStepsFor(
      result(),
      context({ canShowImported: false, canReviewUncategorized: false })
    );
    expect(steps).toEqual(['rulePack', 'balance']);
  });

  it('offers the balance only when it differs from the account balance', () => {
    expect(nextStepsFor(result({ lastBalance: '1000' }), context())).not.toContain('balance');
    expect(nextStepsFor(result({ lastBalance: '1000.00' }), context())).not.toContain('balance');
    expect(nextStepsFor(result({ lastBalance: '1000.01' }), context())).toContain('balance');
  });

  it('waits for the account balance before comparing', () => {
    expect(nextStepsFor(result(), context({ accountBalance: null }))).not.toContain('balance');
  });
});

describe('balanceDiffers', () => {
  it('compares numerically, to the cent', () => {
    expect(balanceDiffers('30196.12', '30196.1200')).toBe(false);
    expect(balanceDiffers('-5', '5')).toBe(true);
    expect(balanceDiffers('0', '0.001')).toBe(false);
  });

  it('is false for a missing or unreadable value', () => {
    expect(balanceDiffers(null, '1')).toBe(false);
    expect(balanceDiffers('1', null)).toBe(false);
    expect(balanceDiffers('abc', '1')).toBe(false);
  });
});

describe('balanceUpdatePayload', () => {
  it('changes only the balance and sends every other field back', () => {
    const account = {
      id: 'acc-1',
      name: 'Main',
      accountType: 'checking',
      iban: 'CZ6508000000192000145399',
      bban: null,
      currency: 'CZK',
      balance: '100',
      institutionId: 'inst_fio',
      externalAccountId: null,
      dataSource: 'manual',
      lastSyncedAt: null,
      interestRate: '1.5',
      hasZoneDesignation: false,
      terminationDate: null,
      interestRateValidUntil: 1_790_000_000,
      excludeFromBalance: false,
      createdAt: 1,
      updatedAt: 2,
    } as BankAccount;
    expect(balanceUpdatePayload(account, '30196.12')).toEqual({
      name: 'Main',
      accountType: 'checking',
      iban: 'CZ6508000000192000145399',
      bban: null,
      currency: 'CZK',
      balance: '30196.12',
      institutionId: 'inst_fio',
      interestRate: '1.5',
      hasZoneDesignation: false,
      terminationDate: null,
      interestRateValidUntil: 1_790_000_000,
      excludeFromBalance: false,
    });
  });
});

describe('formatBalance', () => {
  it('keeps the cents of the number that will be stored, even for CZK', () => {
    expect(formatBalance('30196.12', 'CZK', 'en-US')).toBe('K\u010d\u00a030,196.12');
    expect(formatBalance('-250', 'USD', 'en-US')).toBe('-$250');
  });

  it('falls back to the raw text for something that is not a number', () => {
    expect(formatBalance('n/a', 'CZK', 'en-US')).toBe('n/a');
  });
});

describe('regionName', () => {
  it('localizes the country of a rule pack', () => {
    expect(regionName('de', 'en-US')).toBe('Germany');
    expect(regionName('de', 'cs-CZ')).toBe('Německo');
  });

  it('falls back to the code for something that is not a region', () => {
    expect(regionName('xx-not-a-region', 'en-US')).toBe('XX-NOT-A-REGION');
  });
});
