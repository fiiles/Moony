import { describe, expect, it } from 'vitest';
import type {
  CsvImportPreview,
  CsvImportResult,
  CsvPreset,
  CsvPreviewResult,
  CsvPreviewRow,
} from '@shared/schema';
import {
  AUTO_VALUE,
  EMPTY_MAPPING,
  ENCODING_CHOICES,
  SAMPLE_CSV,
  balanceDiffers,
  buildImportConfig,
  dateFormatChoices,
  dateFormatLabel,
  encodingArgument,
  fileNameFromPath,
  formatBalance,
  formatPreviewAmount,
  groupPresetsByCountry,
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
      '%d %b %Y',
      '%b %d, %Y',
      '%d. %B %Y',
      '%Y年%m月%d日',
      '%Y년 %m월 %d일',
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
    counterpartyIban: null,
    counterpartyBankCode: null,
    counterpartyCountry: null,
    transactionId: null,
    rowBalance: null,
    canImportAnyway: status === 'duplicate',
    bookingDate: 1_700_000_000,
    amount: '-10',
    currency: 'CZK',
    description: null,
    counterparty: null,
    status,
    message: null,
    categoryId: null,
    categorySource: null,
    sourceCategory: null,
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
    promotedCount: 0,
    forcedDuplicateCount: 0,
    offset: 0,
    limit: 200,
    okCount: 1,
    errorCount: 1,
    duplicateCount: 2,
    skippedCount: 1,
    dateRange: null,
    categorizedCount: 0,
    overlap: null,
    ...overrides,
  });

  it('splits the rows to write into categorized and uncategorized', () => {
    expect(summarizePreview(preview({ okCount: 5, categorizedCount: 3 }), [])).toMatchObject({
      willImport: 5,
      categorized: 3,
      uncategorized: 2,
    });
    // A forced duplicate counts in both totals the backend already includes it in.
    expect(
      summarizePreview(preview({ categorizedCount: 2, forcedDuplicateCount: 1 }), [3])
    ).toMatchObject({
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
    expect(summarizePreview(preview({ forcedDuplicateCount: 1 }), [4])).toMatchObject({
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
      summarizePreview(
        preview({ okCount: 500, duplicateCount: 40, totalRows: 560, forcedDuplicateCount: 1 }),
        [3]
      )
    ).toMatchObject({ willImport: 501, skippedDuplicates: 39 });
  });
});

describe('formatPreviewAmount', () => {
  it('signs credits with a plus and debits with a real minus', () => {
    expect(formatPreviewAmount('45000', 'CZK', 'en-US', 'CZK')).toBe('+K\u010d\u00a045,000');
    expect(formatPreviewAmount('-1234.5', 'USD', 'en-US', 'USD')).toBe('\u2212$1,234.50');
  });

  it('shows a dash when the row has no amount and the raw text when it is not a number', () => {
    expect(formatPreviewAmount(null, 'CZK', 'en-US', 'CZK')).toBe('—');
    expect(formatPreviewAmount('abc', 'CZK', 'en-US', 'CZK')).toBe('abc');
  });

  it('never rounds away cents, not even for CZK (which the app shows without decimals)', () => {
    expect(formatPreviewAmount('-250.5', 'CZK', 'en-US', 'CZK')).toBe('\u2212K\u010d\u00a0250.50');
    expect(formatPreviewAmount('0.05', 'CZK', 'en-US', 'CZK')).toBe('+K\u010d\u00a00.05');
    expect(formatPreviewAmount('12.345', 'USD', 'en-US', 'USD')).toBe('+$12.345');
  });

  it('formats without a currency code when the row has none', () => {
    expect(formatPreviewAmount('12.5', null, 'en-US', undefined)).toBe('+12.50');
  });

  it('shows the ISO code for a currency other than the main one', () => {
    expect(formatPreviewAmount('-1234.5', 'USD', 'en-US', 'CZK')).toBe('\u2212USD\u00a01,234.50');
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
    promotedCount: 0,
    statementBalance: {
      amount: '30196.12',
      currency: 'CZK',
      asOf: 1759000000,
      line: 4,
      batchId: 'b',
    },
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
    accountBalanceAsOf: 1759000000,
    ...overrides,
  });

  it('offers everything that applies, in the documented order', () => {
    expect(nextStepsFor(result(), context())).toEqual(['show', 'review', 'rulePack', 'balance']);
  });

  it('offers nothing when nothing applies', () => {
    expect(
      nextStepsFor(
        result({
          uncategorizedCount: 0,
          suggestedRulePack: null,
          lastBalance: null,
          statementBalance: null,
        }),
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

  const statement = (amount: string, asOf = 1759000000): Partial<CsvImportResult> => ({
    statementBalance: { amount, currency: 'CZK', asOf, line: 4, batchId: 'b' },
  });

  it('offers nothing new only for the same day and the same amount as the current balance', () => {
    expect(nextStepsFor(result(statement('1000')), context())).not.toContain('balance');
    expect(nextStepsFor(result(statement('1000.00')), context())).not.toContain('balance');
    // Same day, another amount: a correction of that day's record
    expect(nextStepsFor(result(statement('1000.01')), context())).toContain('balance');
  });

  it('offers an older statement day with a different amount as history', () => {
    const older = 1759000000 - 30 * 86400;
    expect(nextStepsFor(result(statement('900', older)), context())).toContain('balance');
    // Even the same amount on another day is a record of its own
    expect(nextStepsFor(result(statement('1000', older)), context())).toContain('balance');
  });

  it('compares the statement day, not the second', () => {
    const dayStart = 20359 * 86400;
    expect(
      nextStepsFor(
        result(statement('1000', dayStart + 3600)),
        context({ accountBalanceAsOf: dayStart })
      )
    ).not.toContain('balance');
  });

  it('offers the first dated record on an undated balance', () => {
    expect(
      nextStepsFor(result(statement('1000')), context({ accountBalanceAsOf: null }))
    ).toContain('balance');
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

describe('formatBalance', () => {
  it('keeps the cents of the number that will be stored, even for CZK', () => {
    expect(formatBalance('30196.12', 'CZK', 'en-US', 'CZK')).toBe('K\u010d\u00a030,196.12');
    expect(formatBalance('-250', 'USD', 'en-US', 'USD')).toBe('-$250');
  });

  it('falls back to the raw text for something that is not a number', () => {
    expect(formatBalance('n/a', 'CZK', 'en-US', 'CZK')).toBe('n/a');
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

it('uses full-file forced counts while previewing another page', () => {
  const preview: CsvImportPreview = {
    rows: [],
    totalRows: 405,
    okCount: 0,
    errorCount: 0,
    duplicateCount: 405,
    skippedCount: 0,
    dateRange: null,
    categorizedCount: 1,
    promotedCount: 0,
    forcedDuplicateCount: 2,
    overlap: null,
    offset: 200,
    limit: 200,
  };
  expect(summarizePreview(preview, [2, 402])).toMatchObject({
    willImport: 2,
    forcedDuplicates: 2,
    skippedDuplicates: 403,
    categorized: 1,
  });
});

it('keeps structured identifier and value-date mapping in the import config', () => {
  const inspection: CsvPreviewResult = {
    headers: [],
    sampleRows: [],
    totalRows: 1,
    delimiter: ';',
    headerRow: 0,
    detectedPresetId: null,
    dateFormat: '%Y-%m-%d',
    dateFormatAmbiguous: false,
    decimalSeparator: '.',
    encoding: 'UTF-8',
    suggestedDescriptionColumns: [],
    suggestedRowFilter: null,
    suggestedMappings: {
      date: ['Date', 1],
      amount: ['Amount', 1],
      valueDate: ['Value', 1],
      counterpartyBankCode: ['Kód banky', 1],
      counterpartyCountry: ['Country', 1],
    },
  };
  const config = buildImportConfig(mappingFromInspection(inspection), inspection, 0);
  expect(config).toMatchObject({
    counterpartyBankCodeColumn: 'Kód banky',
    counterpartyCountryColumn: 'Country',
  });
  // The value date is still detected (so it is not taken for the date) but never imported.
  expect(config).not.toHaveProperty('valueDateColumn');
});

it('round-trips manually mapped advanced columns and clears them back to null', () => {
  const ins = {
    delimiter: ';',
    headerRow: 0,
    encoding: 'UTF-8',
    suggestedRowFilter: null,
  };
  const manual = {
    ...EMPTY_MAPPING,
    dateColumn: 'Date',
    amountColumn: 'Amount',
    counterpartyBankCodeColumn: 'Kód banky protiúčtu',
    counterpartyCountryColumn: 'Země',
  };
  expect(buildImportConfig(manual, ins, 0)).toMatchObject({
    counterpartyBankCodeColumn: 'Kód banky protiúčtu',
    counterpartyCountryColumn: 'Země',
  });
  const cleared = {
    ...manual,
    counterpartyBankCodeColumn: '',
    counterpartyCountryColumn: '',
  };
  expect(buildImportConfig(cleared, ins, 0)).toMatchObject({
    counterpartyBankCodeColumn: null,
    counterpartyCountryColumn: null,
  });
});

describe('encoding override', () => {
  it('offers the encodings the import dialog documents, Auto handled separately', () => {
    expect(ENCODING_CHOICES.map((choice) => choice.value)).toEqual([
      'UTF-8',
      'windows-1250',
      'windows-1251',
      'windows-1252',
      'windows-1253',
      'windows-1254',
      'windows-1257',
      'ISO-8859-1',
      'ISO-8859-2',
      'Shift_JIS',
      'GBK',
      'Big5',
      'EUC-KR',
    ]);
  });

  it('sends nothing for Auto and the chosen name otherwise', () => {
    expect(encodingArgument(AUTO_VALUE)).toBeUndefined();
    expect(encodingArgument('Shift_JIS')).toBe('Shift_JIS');
  });

  it('writes the encoding the file was read with into the preview and import config', () => {
    const config = buildImportConfig(mapping(), inspection({ encoding: 'Shift_JIS' }), 0);
    expect(config.encoding).toBe('Shift_JIS');
  });
});

describe('groupPresetsByCountry', () => {
  const preset = (id: string, bankName: string, country: string) =>
    ({ id, bankName, country, institutionId: null }) as CsvPreset;
  const presets = [
    preset('chase', 'Chase', 'US'),
    preset('dkb', 'DKB', 'DE'),
    preset('air', 'Air Bank', 'CZ'),
    preset('revolut', 'Revolut', 'GB'),
    preset('csob', 'ČSOB', 'CZ'),
    preset('n26', 'N26', 'DE'),
  ];

  it('puts the profile country first and the rest in the order of their localized names', () => {
    const groups = groupPresetsByCountry(presets, 'de', 'en-US');
    expect(groups.map((g) => g.country)).toEqual(['DE', 'CZ', 'GB', 'US']);
    expect(groups.map((g) => g.name)).toEqual([
      'Germany',
      'Czechia',
      'United Kingdom',
      'United States',
    ]);
    expect(groups[0].presets.map((p) => p.id)).toEqual(['dkb', 'n26']);
    expect(groups[1].presets.map((p) => p.id)).toEqual(['air', 'csob']);
  });

  it('orders by the language of the interface when the country is unknown', () => {
    expect(groupPresetsByCountry(presets, null, 'cs-CZ').map((g) => g.name)).toEqual([
      'Česko',
      'Německo',
      'Spojené království',
      'Spojené státy',
    ]);
  });

  it('ignores the case of the profile country and an unknown one', () => {
    expect(groupPresetsByCountry(presets, 'us', 'en-US')[0].country).toBe('US');
    expect(groupPresetsByCountry(presets, 'XX', 'en-US')[0].country).toBe('CZ');
    expect(groupPresetsByCountry([], 'US', 'en-US')).toEqual([]);
  });
});

describe('date formats with month names and CJK units', () => {
  it('are offered with notation labels', () => {
    expect(dateFormatLabel('%d %b %Y')).toBe('DD MMM YYYY');
    expect(dateFormatLabel('%b %d, %Y')).toBe('MMM DD, YYYY');
    expect(dateFormatLabel('%d. %B %Y')).toBe('DD. MMMM YYYY');
    expect(dateFormatLabel('%Y年%m月%d日')).toBe('YYYY年M月D日');
    expect(dateFormatLabel('%Y년 %m월 %d일')).toBe('YYYY년 M월 D일');
    expect(dateFormatChoices()).toEqual(
      expect.arrayContaining([
        '%d %b %Y',
        '%b %d, %Y',
        '%d. %B %Y',
        '%Y年%m月%d日',
        '%Y년 %m월 %d일',
      ])
    );
  });
});
