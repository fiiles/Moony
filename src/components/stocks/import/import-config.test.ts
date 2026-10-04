import { describe, expect, it } from 'vitest';
import type {
  StockCsvInspection,
  StockImportBatch,
  StockImportConfig,
  StockImportInstrument,
  StockImportPreview,
  StockInstrumentCandidate,
  StockInstrumentOverride,
  StockInstrumentResolution,
  StockPreviewRow,
} from '@shared/schema';
import {
  AUTO_VALUE,
  EMPTY_MAPPING,
  autoOverrideFor,
  buildSampleCsv,
  buildStockImportConfig,
  classifyResultMessage,
  columnOptions,
  columnToSelectValue,
  dateFormatChoices,
  dateFormatLabel,
  directionModePatch,
  fileStem,
  findTypeValue,
  formatConfig,
  formatDecimalText,
  groupResultMessages,
  groupTypeValues,
  hasOverrideContent,
  hasTradeValue,
  incompleteFields,
  instrumentCheck,
  instrumentQuery,
  isCurrencyCode,
  isDateFormatGuess,
  isMappingComplete,
  isSavedFormatSource,
  isSkipped,
  isSourceId,
  isValidTicker,
  mappedColumns,
  mappingFromInspection,
  mergeAutoOverrides,
  missingSymbolCount,
  needsLookup,
  normalizeTicker,
  optionLabel,
  rowStatusByLine,
  selectValueToColumn,
  setTypeValueAction,
  sourceHelpUrl,
  sourceMismatch,
  suggestedColumn,
  suggestionConfidence,
  summarizeMapping,
  summarizePreview,
  typeColumnPatch,
  typeValueAction,
  typeValueRows,
  typeValuesForColumn,
  undoableBatches,
  updateOverride,
  type ColumnLabels,
  type MappingState,
} from './import-config';

const labels: ColumnLabels = {
  blank: (position) => `Column ${position}`,
  blankAfter: (position, after) => `Column ${position} (after "${after}")`,
};

const config = (overrides: Partial<StockImportConfig> = {}): StockImportConfig => ({
  source: 'trading212',
  delimiter: ',',
  encoding: 'UTF-8',
  headerRow: 0,
  skipRows: 0,
  dateColumn: 1,
  dateFormat: '%Y-%m-%d',
  symbolColumn: 3,
  isinColumn: 2,
  nameColumn: 4,
  quantityColumn: 5,
  priceColumn: 6,
  currencyMode: 'column',
  currencyColumn: 7,
  fixedCurrency: null,
  directionMode: 'typeColumn',
  typeColumn: 0,
  typeValues: [
    { value: 'Market buy', action: 'buy' },
    { value: 'Market sell', action: 'sell' },
    { value: 'Deposit', action: 'skip' },
  ],
  decimalSeparator: '.',
  externalIdColumn: 8,
  transforms: {},
  instrumentOverrides: [],
  importAnywayLines: [],
  ...overrides,
});

const inspection = (overrides: Partial<StockCsvInspection> = {}): StockCsvInspection => ({
  fileName: 'history.csv',
  encoding: 'UTF-8',
  delimiter: ',',
  headerRow: 0,
  headers: [
    'Action',
    'Time',
    'ISIN',
    'Ticker',
    'Name',
    'No. of shares',
    'Price / share',
    'Cur',
    'ID',
  ],
  sampleRows: [],
  rowCount: 3,
  detectedSource: null,
  config: null,
  suggestions: [
    { role: 'date', column: 1, confidence: 0.95 },
    { role: 'type', column: 0, confidence: 0.85 },
    { role: 'symbol', column: 3, confidence: 0.95 },
    { role: 'isin', column: 2, confidence: 1 },
    { role: 'quantity', column: 5, confidence: 0.85 },
    { role: 'price', column: 6, confidence: 0.85 },
    { role: 'currency', column: 7, confidence: 0.6 },
  ],
  columnValues: [
    {
      column: 0,
      values: [
        { value: 'Market buy', count: 2, firstLine: 2, suggested: 'buy' },
        { value: 'Market sell', count: 1, firstLine: 4, suggested: 'sell' },
        { value: 'Deposit', count: 1, firstLine: 3, suggested: 'skip' },
      ],
    },
  ],
  dateFormat: '%Y-%m-%d',
  dateFormatAmbiguous: false,
  decimalSeparator: '.',
  hasFeeColumn: false,
  ...overrides,
});

const mapping = (overrides: Partial<MappingState> = {}): MappingState => ({
  ...mappingFromInspection(inspection({ config: config() })),
  ...overrides,
});

const instrument = (overrides: Partial<StockImportInstrument> = {}): StockImportInstrument => ({
  key: 'symbol:AAPL',
  symbol: 'AAPL',
  isin: null,
  name: null,
  currency: 'USD',
  tradeCount: 2,
  ticker: 'AAPL',
  status: 'new',
  positionCurrency: null,
  ...overrides,
});

const candidate = (
  symbol: string,
  currency: string,
  overrides: Partial<StockInstrumentCandidate> = {}
): StockInstrumentCandidate => ({
  symbol,
  name: `${symbol} Inc.`,
  exchange: 'XXX',
  currency,
  ...overrides,
});

const resolution = (
  candidates: StockInstrumentCandidate[],
  overrides: Partial<StockInstrumentResolution> = {}
): StockInstrumentResolution => ({
  key: 'symbol:AAPL',
  candidates,
  best: candidates[0] ?? null,
  lookupFailed: false,
  ...overrides,
});

describe('sources', () => {
  it('knows the list entries and saved formats', () => {
    expect(isSourceId('xtb')).toBe(true);
    expect(isSourceId('custom')).toBe(true);
    expect(isSourceId('format:abc')).toBe(false);
    expect(isSourceId(null)).toBe(false);
    expect(isSavedFormatSource('format:abc')).toBe(true);
    expect(isSavedFormatSource('xtb')).toBe(false);
    expect(isSavedFormatSource(null)).toBe(false);
  });

  it('links brokers to a help page in the UI language and the rest to none', () => {
    expect(sourceHelpUrl('xtb', 'cs')).toContain('/cz/');
    expect(sourceHelpUrl('xtb', 'en')).toContain('/en/');
    expect(sourceHelpUrl('degiro', 'cs')).toContain('degiro.cz');
    expect(sourceHelpUrl('ibkr', 'en')).toContain('ibkrguides.com');
    expect(sourceHelpUrl('trading212', 'cs')).toContain('trading212.com');
    expect(sourceHelpUrl('moony', 'en')).toBeNull();
    expect(sourceHelpUrl('custom', 'cs')).toBeNull();
    expect(sourceHelpUrl('format:abc', 'cs')).toBeNull();
  });
});

describe('sourceMismatch', () => {
  it('claims nothing when the file is what the user picked', () => {
    expect(sourceMismatch('xtb', 'xtb')).toBe('none');
    expect(sourceMismatch('format:abc', 'format:abc')).toBe('none');
  });

  it('claims nothing when the user picked nothing or "other broker"', () => {
    expect(sourceMismatch(null, 'degiro')).toBe('none');
    expect(sourceMismatch('custom', 'degiro')).toBe('none');
    expect(sourceMismatch('custom', null)).toBe('none');
  });

  it('tells a file of another source from one that is none of them', () => {
    expect(sourceMismatch('xtb', 'degiro')).toBe('other');
    expect(sourceMismatch('moony', 'format:abc')).toBe('other');
    expect(sourceMismatch('xtb', null)).toBe('unknown');
  });
});

describe('columnOptions', () => {
  const headers = ['Date', 'Price', '', 'Value', '', 'Value'];
  const rows = [
    ['2024-01-15', '10.5', 'EUR', '105', '', 'a-very-long-example-value-that-is-cut'],
    ['2024-01-16', '10.5', 'EUR', '210', '', ''],
    ['2024-01-17', '11', 'USD', '', '', ''],
    ['2024-01-18', '12', 'GBP', '', '', ''],
  ];

  it('labels blank headers by position and by the header they follow', () => {
    const options = columnOptions(headers, rows, labels);
    expect(options[0].label).toBe('Date');
    expect(options[2].label).toBe('Column 3 (after "Price")');
    expect(options[4].label).toBe('Column 5 (after "Value")');
    expect(columnOptions(['', 'A'], [], labels)[0].label).toBe('Column 1');
  });

  it('tells repeated headers apart by their position', () => {
    const options = columnOptions(headers, rows, labels);
    expect(options[3].label).toBe('Value');
    expect(options[5].label).toBe('Value (6)');
  });

  it('shows up to three distinct values, cut to a readable length', () => {
    const options = columnOptions(headers, rows, labels);
    expect(options[0].example).toBe('2024-01-15, 2024-01-16, 2024-01-17');
    expect(options[1].example).toBe('10.5, 11, 12');
    expect(options[2].example).toBe('EUR, USD, GBP');
    expect(options[4].example).toBe('');
    expect(options[5].example).toBe('a-very-long-example-v…');
  });

  it('finds the label of a column index', () => {
    const options = columnOptions(headers, rows, labels);
    expect(optionLabel(options, 1)).toBe('Price');
    expect(optionLabel(options, null)).toBe('');
    expect(optionLabel(options, 99)).toBe('');
  });

  it('converts between indexes and select values', () => {
    expect(columnToSelectValue(null)).toBe('__none__');
    expect(columnToSelectValue(0)).toBe('0');
    expect(selectValueToColumn('__none__')).toBeNull();
    expect(selectValueToColumn('')).toBeNull();
    expect(selectValueToColumn('7')).toBe(7);
    expect(selectValueToColumn('x')).toBeNull();
  });
});

describe('mappingFromInspection', () => {
  it('takes a detected source over as it is', () => {
    const m = mappingFromInspection(inspection({ detectedSource: 'trading212', config: config() }));
    expect(m).toMatchObject({
      source: 'trading212',
      dateColumn: 1,
      dateFormat: '%Y-%m-%d',
      symbolColumn: 3,
      isinColumn: 2,
      nameColumn: 4,
      quantityColumn: 5,
      priceColumn: 6,
      currencyMode: 'column',
      currencyColumn: 7,
      directionMode: 'typeColumn',
      typeColumn: 0,
      externalIdColumn: 8,
    });
    expect(m.typeValues).toEqual(config().typeValues);
  });

  it('copies the type values instead of sharing them with the inspection', () => {
    const insp = inspection({ config: config() });
    const m = mappingFromInspection(insp);
    m.typeValues[0].action = 'sell';
    expect(insp.config?.typeValues?.[0].action).toBe('buy');
  });

  it('follows the detected decimal separator unless the config differs from it', () => {
    expect(mappingFromInspection(inspection({ config: config() })).decimalSeparator).toBe(
      AUTO_VALUE
    );
    expect(
      mappingFromInspection(inspection({ config: config({ decimalSeparator: ',' }) }))
        .decimalSeparator
    ).toBe(',');
  });

  it('builds a manual mapping from the suggestions when there is no config', () => {
    const m = mappingFromInspection(inspection());
    expect(m).toMatchObject({
      source: 'custom',
      dateColumn: 1,
      dateFormat: '%Y-%m-%d',
      symbolColumn: 3,
      isinColumn: 2,
      quantityColumn: 5,
      priceColumn: 6,
      currencyMode: 'column',
      currencyColumn: 7,
      directionMode: 'typeColumn',
      typeColumn: 0,
      decimalSeparator: AUTO_VALUE,
    });
    expect(m.typeValues).toEqual([
      { value: 'Market buy', action: 'buy' },
      { value: 'Market sell', action: 'sell' },
      { value: 'Deposit', action: 'skip' },
    ]);
  });

  it('reads the direction from the quantity sign and the currency from the listing when no column is suggested', () => {
    const m = mappingFromInspection(
      inspection({
        suggestions: [
          { role: 'date', column: 1, confidence: 1 },
          { role: 'symbol', column: 3, confidence: 1 },
          { role: 'quantity', column: 5, confidence: 1 },
          { role: 'price', column: 6, confidence: 1 },
        ],
        dateFormat: null,
      })
    );
    expect(m.directionMode).toBe('quantitySign');
    expect(m.typeColumn).toBeNull();
    expect(m.currencyMode).toBe('instrument');
    expect(m.dateFormat).toBe('');
    expect(isMappingComplete(m)).toBe(false);
    expect(incompleteFields(m)).toEqual(['dateFormat']);
  });

  it('prefers the more confident of two suggestions for a role', () => {
    const m = mappingFromInspection(
      inspection({
        suggestions: [
          { role: 'date', column: 4, confidence: 0.85 },
          { role: 'date', column: 1, confidence: 0.95 },
        ],
      })
    );
    expect(m.dateColumn).toBe(1);
  });

  it('lists the values of a column only when the inspection counted them', () => {
    const insp = inspection();
    expect(typeValuesForColumn(insp, 0)).toHaveLength(3);
    expect(typeValuesForColumn(insp, 5)).toEqual([]);
    expect(typeValuesForColumn(insp, null)).toEqual([]);
  });
});

describe('type column and direction mode', () => {
  const insp = inspection();

  it('lists the values of another type column with their suggested meaning', () => {
    expect(typeColumnPatch(insp, 0)).toEqual({
      typeColumn: 0,
      typeValues: [
        { value: 'Market buy', action: 'buy' },
        { value: 'Market sell', action: 'sell' },
        { value: 'Deposit', action: 'skip' },
      ],
    });
    expect(typeColumnPatch(insp, 5)).toEqual({ typeColumn: 5, typeValues: [] });
    expect(typeColumnPatch(insp, null)).toEqual({ typeColumn: null, typeValues: [] });
  });

  it('finds the suggested column of a role', () => {
    expect(suggestedColumn(insp.suggestions, 'type')).toBe(0);
    expect(suggestedColumn(insp.suggestions, 'name')).toBeNull();
  });

  it('keeps the type column when the sign decides, so switching back loses nothing', () => {
    expect(directionModePatch(insp, { typeColumn: 3 }, 'quantitySign')).toEqual({
      directionMode: 'quantitySign',
    });
    expect(directionModePatch(insp, { typeColumn: 3 }, 'typeColumn')).toEqual({
      directionMode: 'typeColumn',
    });
  });

  it('picks the suggested type column when there is none yet', () => {
    const patch = directionModePatch(insp, { typeColumn: null }, 'typeColumn');
    expect(patch.directionMode).toBe('typeColumn');
    expect(patch.typeColumn).toBe(0);
    expect(patch.typeValues).toHaveLength(3);
  });
});

describe('suggestionConfidence', () => {
  const insp = inspection();

  it('shows the confidence of the column that is still chosen', () => {
    expect(suggestionConfidence(insp, mapping(), 'date')).toEqual({ level: 'high', percent: 95 });
    expect(suggestionConfidence(insp, mapping(), 'type')).toEqual({
      level: 'medium',
      percent: 85,
    });
  });

  it('shows nothing below 70 %, for another column, or for an unused role', () => {
    expect(suggestionConfidence(insp, mapping(), 'currency')).toBeNull();
    expect(suggestionConfidence(insp, mapping({ dateColumn: 4 }), 'date')).toBeNull();
    expect(suggestionConfidence(insp, mapping({ nameColumn: null }), 'name')).toBeNull();
  });
});

describe('isMappingComplete', () => {
  it('is complete for a detected source', () => {
    expect(isMappingComplete(mapping())).toBe(true);
    expect(incompleteFields(mapping())).toEqual([]);
  });

  it('lists what is missing in the order of the form', () => {
    const m = mapping({
      dateColumn: null,
      dateFormat: '',
      symbolColumn: null,
      isinColumn: null,
      quantityColumn: null,
      priceColumn: null,
      currencyColumn: null,
      typeColumn: null,
    });
    expect(incompleteFields(m)).toEqual([
      'date',
      'dateFormat',
      'symbol',
      'quantity',
      'price',
      'currency',
      'type',
    ]);
  });

  it('is satisfied with an ISIN column alone', () => {
    expect(isMappingComplete(mapping({ symbolColumn: null }))).toBe(true);
    expect(isMappingComplete(mapping({ symbolColumn: null, isinColumn: null }))).toBe(false);
  });

  it('asks for a valid code when one currency applies to the whole file', () => {
    expect(isMappingComplete(mapping({ currencyMode: 'fixed', fixedCurrency: '' }))).toBe(false);
    expect(isMappingComplete(mapping({ currencyMode: 'fixed', fixedCurrency: 'dollar' }))).toBe(
      false
    );
    expect(isMappingComplete(mapping({ currencyMode: 'fixed', fixedCurrency: ' usd ' }))).toBe(
      true
    );
    expect(isCurrencyCode('EUR')).toBe(true);
    expect(isCurrencyCode('EU')).toBe(false);
  });

  it('needs no currency column in listing mode and no type column when the sign decides', () => {
    expect(isMappingComplete(mapping({ currencyMode: 'instrument', currencyColumn: null }))).toBe(
      true
    );
    expect(isMappingComplete(mapping({ directionMode: 'quantitySign', typeColumn: null }))).toBe(
      true
    );
  });
});

describe('buildStockImportConfig', () => {
  const settings = {
    delimiter: ';',
    encoding: 'windows-1250',
    headerRow: 2,
    decimalSeparator: ',',
  };

  it('builds the config of a complete mapping', () => {
    const built = buildStockImportConfig(mapping(), settings, { skipRows: 1 });
    expect(built).toEqual({
      source: 'trading212',
      delimiter: ';',
      encoding: 'windows-1250',
      headerRow: 2,
      skipRows: 1,
      dateColumn: 1,
      dateFormat: '%Y-%m-%d',
      symbolColumn: 3,
      isinColumn: 2,
      nameColumn: 4,
      quantityColumn: 5,
      priceColumn: 6,
      currencyMode: 'column',
      currencyColumn: 7,
      fixedCurrency: null,
      directionMode: 'typeColumn',
      typeColumn: 0,
      typeValues: config().typeValues,
      decimalSeparator: ',',
      externalIdColumn: 8,
      transforms: {},
      instrumentOverrides: [],
      importAnywayLines: [],
    });
  });

  it('returns null while the mapping is incomplete', () => {
    expect(buildStockImportConfig(mapping({ quantityColumn: null }), settings)).toBeNull();
  });

  it('keeps only what the chosen modes use', () => {
    const fixed = buildStockImportConfig(
      mapping({ currencyMode: 'fixed', fixedCurrency: ' usd ', currencyColumn: 7 }),
      settings
    );
    expect(fixed).toMatchObject({
      currencyMode: 'fixed',
      fixedCurrency: 'USD',
      currencyColumn: null,
    });

    const bySign = buildStockImportConfig(
      mapping({ directionMode: 'quantitySign', typeColumn: 0 }),
      settings
    );
    expect(bySign).toMatchObject({
      directionMode: 'quantitySign',
      typeColumn: null,
      typeValues: [],
    });

    const column = buildStockImportConfig(mapping({ fixedCurrency: 'EUR' }), settings);
    expect(column).toMatchObject({ currencyMode: 'column', fixedCurrency: null });
  });

  it('resolves the automatic decimal separator from the inspection', () => {
    expect(
      buildStockImportConfig(mapping({ decimalSeparator: AUTO_VALUE }), {
        ...settings,
        decimalSeparator: '.',
      })?.decimalSeparator
    ).toBe('.');
    expect(
      buildStockImportConfig(mapping({ decimalSeparator: ',' }), {
        ...settings,
        decimalSeparator: '.',
      })?.decimalSeparator
    ).toBe(',');
  });

  it('adds the review choices: sorted duplicate lines and overrides that change something', () => {
    const overrides: StockInstrumentOverride[] = [
      { key: 'isin:A', ticker: 'ASML.AS' },
      { key: 'isin:B', skip: false },
      { key: 'isin:C', skip: true },
    ];
    const built = buildStockImportConfig(mapping(), settings, {
      importAnywayLines: [9, 3, 5],
      instrumentOverrides: overrides,
    });
    expect(built?.importAnywayLines).toEqual([3, 5, 9]);
    expect(built?.instrumentOverrides).toEqual([
      { key: 'isin:A', ticker: 'ASML.AS' },
      { key: 'isin:C', skip: true },
    ]);
  });

  it('carries the adapter transforms', () => {
    const built = buildStockImportConfig(
      mapping({ transforms: { xtbComment: true, xtbSymbols: true } }),
      settings
    );
    expect(built?.transforms).toEqual({ xtbComment: true, xtbSymbols: true });
  });

  it('leaves what belongs to one file out of a remembered format', () => {
    const built = buildStockImportConfig(mapping(), settings, {
      skipRows: 4,
      importAnywayLines: [3],
      instrumentOverrides: [{ key: 'isin:A', ticker: 'ASML.AS' }],
    }) as StockImportConfig;
    const saved = formatConfig(built);
    expect(saved).toMatchObject({
      skipRows: 0,
      importAnywayLines: [],
      instrumentOverrides: [],
      dateColumn: 1,
    });
    expect(built.skipRows).toBe(4);
  });
});

describe('type values', () => {
  const values = config().typeValues ?? [];

  it('reads a listed value and skips an unlisted one', () => {
    expect(typeValueAction(values, 'Market buy')).toBe('buy');
    expect(typeValueAction(values, 'Market sell')).toBe('sell');
    expect(typeValueAction(values, 'Dividend')).toBe('skip');
  });

  it('falls back to the same word in another spelling', () => {
    const listed = [
      { value: 'buy', action: 'buy' as const },
      { value: 'prodej', action: 'sell' as const },
    ];
    expect(typeValueAction(listed, 'Buy')).toBe('buy');
    expect(typeValueAction(listed, ' BUY ')).toBe('buy');
    expect(typeValueAction(listed, 'Prodéj')).toBe('sell');
    expect(typeValueAction(listed, 'Dividend')).toBe('skip');
  });

  it('prefers the exact value over the fallback', () => {
    const listed = [
      { value: 'Buy', action: 'sell' as const },
      { value: 'buy', action: 'buy' as const },
    ];
    expect(typeValueAction(listed, 'Buy')).toBe('sell');
    expect(typeValueAction(listed, 'buy')).toBe('buy');
  });

  it('changes a value in place and adds a new one', () => {
    const changed = setTypeValueAction(values, 'Deposit', 'buy');
    expect(changed.map((v) => v.action)).toEqual(['buy', 'sell', 'buy']);
    const added = setTypeValueAction(values, 'Dividend', 'skip');
    expect(added).toHaveLength(4);
    expect(values).toHaveLength(3);
  });

  it('knows whether anything would be read as a trade', () => {
    expect(hasTradeValue(values)).toBe(true);
    expect(hasTradeValue([{ value: 'Deposit', action: 'skip' }])).toBe(false);
    expect(hasTradeValue([])).toBe(false);
  });

  it('groups the values of the file by what they mean', () => {
    const groups = groupTypeValues(mapping(), inspection());
    expect(groups).toEqual({
      buy: ['Market buy'],
      sell: ['Market sell'],
      skip: ['Deposit'],
    });
  });

  it('groups only the values of the file and skips the ones the mapping lacks', () => {
    const m = mapping({ typeValues: [{ value: 'Market buy', action: 'buy' }] });
    expect(groupTypeValues(m, inspection())).toEqual({
      buy: ['Market buy'],
      sell: [],
      skip: ['Market sell', 'Deposit'],
    });
  });

  it('groups what the mapping lists when the column was not counted', () => {
    // a heavy user's action column has more than 30 distinct values: no counts, the mapping is complete
    expect(groupTypeValues(mapping(), inspection({ columnValues: [] }))).toEqual({
      buy: ['Market buy'],
      sell: ['Market sell'],
      skip: ['Deposit'],
    });
  });

  it('has no groups when the sign decides the direction', () => {
    expect(groupTypeValues(mapping({ directionMode: 'quantitySign' }), inspection())).toEqual({
      buy: [],
      sell: [],
      skip: [],
    });
  });
});

describe('type value rows', () => {
  const entries = [
    { value: 'Market buy', action: 'buy' as const },
    { value: 'Limit buy', action: 'buy' as const },
    { value: 'Market sell', action: 'sell' as const },
    { value: 'Deposit', action: 'skip' as const },
  ];

  it('finds an entry by the value as written, then by the same word in another spelling', () => {
    expect(findTypeValue(entries, 'Market buy')?.value).toBe('Market buy');
    expect(findTypeValue(entries, ' MARKET BUY ')?.value).toBe('Market buy');
    expect(findTypeValue(entries, 'Dividend')).toBeUndefined();
  });

  it('lists the values of the file, most frequent first, with their counts', () => {
    const rows = typeValueRows(entries, [
      { value: 'Deposit', count: 9, firstLine: 7, suggested: 'skip' },
      { value: 'market buy', count: 4, firstLine: 2, suggested: 'buy' },
      { value: 'Dividend', count: 1, firstLine: 12, suggested: 'skip' },
    ]);
    expect(rows).toEqual([
      { value: 'Deposit', entryValue: 'Deposit', action: 'skip', count: 9, firstLine: 7 },
      // another spelling of an entry: shown as the file writes it, changed through the entry
      { value: 'market buy', entryValue: 'Market buy', action: 'buy', count: 4, firstLine: 2 },
      // a value the mapping does not list is skipped
      { value: 'Dividend', entryValue: 'Dividend', action: 'skip', count: 1, firstLine: 12 },
    ]);
  });

  it('leaves out what the mapping knows and the file does not once the file was counted', () => {
    const rows = typeValueRows(entries, [
      { value: 'Market buy', count: 3, firstLine: 2, suggested: 'buy' },
    ]);
    expect(rows.map((r) => r.value)).toEqual(['Market buy']);
  });

  it('is the mapping itself, without counts, for a column that was not counted', () => {
    const rows = typeValueRows(entries, undefined);
    expect(rows.map((r) => r.value)).toEqual(['Market buy', 'Limit buy', 'Market sell', 'Deposit']);
    expect(rows.every((r) => r.count === null && r.firstLine === null)).toBe(true);
    expect(rows[2]).toMatchObject({ entryValue: 'Market sell', action: 'sell' });
    expect(typeValueRows(entries, [])).toHaveLength(4);
    expect(typeValueRows([], undefined)).toEqual([]);
  });
});

describe('summarizeMapping', () => {
  const options = columnOptions(inspection().headers, [], labels);

  it('lists what is read from which column', () => {
    const items = summarizeMapping(mapping(), options);
    expect(items.map((i) => `${i.role}:${i.kind}:${i.value}`)).toEqual([
      'date:column:Time',
      'direction:column:Action',
      'symbol:column:Ticker',
      'isin:column:ISIN',
      'name:column:Name',
      'quantity:column:No. of shares',
      'price:column:Price / share',
      'currency:column:Cur',
      'externalId:column:ID',
    ]);
  });

  it('describes a direction from the sign, a fixed currency and one from the listing', () => {
    const sign = summarizeMapping(
      mapping({ directionMode: 'quantitySign', currencyMode: 'fixed', fixedCurrency: 'usd' }),
      options
    );
    expect(sign).toContainEqual({ role: 'direction', kind: 'sign', value: '' });
    expect(sign).toContainEqual({ role: 'currency', kind: 'fixed', value: 'USD' });
    const listing = summarizeMapping(mapping({ currencyMode: 'instrument' }), options);
    expect(listing).toContainEqual({ role: 'currency', kind: 'instrument', value: '' });
  });

  it('reads quantity and price from one comment column for XTB', () => {
    const items = summarizeMapping(
      mapping({ quantityColumn: 4, priceColumn: 4, transforms: { xtbComment: true } }),
      options
    );
    expect(items).toContainEqual({ role: 'quantityPrice', kind: 'comment', value: 'Name' });
    expect(items.some((i) => i.role === 'quantity' || i.role === 'price')).toBe(false);
  });

  it('highlights every mapped column with its roles', () => {
    const map = mappedColumns(mapping({ quantityColumn: 5, priceColumn: 5 }));
    expect(map.get(5)).toEqual(['quantity', 'price']);
    expect(map.get(1)).toEqual(['date']);
    expect(map.has(8)).toBe(true);
    expect(map.has(99)).toBe(false);
    expect(mappedColumns(mapping({ directionMode: 'quantitySign' })).has(0)).toBe(false);
    expect(mappedColumns(mapping({ currencyMode: 'instrument' })).has(7)).toBe(false);
  });
});

describe('date formats', () => {
  it('offers the known formats and adds a detected one that is not among them', () => {
    expect(dateFormatChoices()).toContain('%Y%m%d');
    expect(dateFormatChoices('%d.%m.%Y')).not.toContain(undefined);
    expect(dateFormatChoices('%Y/%m/%d', null, undefined).at(-1)).toBe('%Y/%m/%d');
    expect(dateFormatChoices('%d.%m.%Y').filter((f) => f === '%d.%m.%Y')).toHaveLength(1);
  });

  it('shows the notation of a format and the raw pattern of an unknown one', () => {
    expect(dateFormatLabel('%d.%m.%Y')).toBe('DD.MM.YYYY');
    expect(dateFormatLabel('%Y%m%d')).toBe('YYYYMMDD');
    expect(dateFormatLabel('%Y/%m/%d')).toBe('%Y/%m/%d');
  });

  it('calls the order a guess only while the detected format is still selected on its column', () => {
    const insp = inspection({ dateFormat: '%d.%m.%Y', dateFormatAmbiguous: true });
    const m = mapping({ dateFormat: '%d.%m.%Y', dateColumn: 1 });
    expect(isDateFormatGuess(insp, m)).toBe(true);
    expect(isDateFormatGuess(insp, { ...m, dateFormat: '%m/%d/%Y' })).toBe(false);
    expect(isDateFormatGuess(insp, { ...m, dateColumn: 4 })).toBe(false);
    expect(isDateFormatGuess({ ...insp, dateFormatAmbiguous: false }, m)).toBe(false);
  });
});

describe('summarizePreview', () => {
  const preview = (counts: Partial<StockImportPreview['counts']> = {}) => ({
    counts: { total: 10, willImport: 6, duplicates: 2, skipped: 1, errors: 1, ...counts },
  });

  it('passes the counts of the whole file through', () => {
    expect(summarizePreview(preview())).toEqual({
      willImport: 6,
      duplicates: 2,
      skipped: 1,
      errors: 1,
      total: 10,
    });
  });

  it('takes the backend at its word about duplicates imported anyway', () => {
    // 7 to import = 6 new + 1 duplicate ticked; the duplicate is no longer counted as left out
    expect(summarizePreview(preview({ willImport: 7, duplicates: 1 }))).toMatchObject({
      willImport: 7,
      duplicates: 1,
    });
  });

  it('counts the instruments that still need a symbol', () => {
    const p = {
      instruments: [
        instrument({ status: 'missingSymbol' }),
        instrument({ key: 'b', status: 'new' }),
        instrument({ key: 'c', status: 'missingSymbol' }),
      ],
    };
    expect(missingSymbolCount(p)).toBe(2);
  });
});

describe('instrument overrides', () => {
  it('adds an override, merges the next change into it and drops it when it says nothing', () => {
    let overrides = updateOverride([], 'isin:A', { ticker: 'ASML.AS' });
    expect(overrides).toEqual([{ key: 'isin:A', ticker: 'ASML.AS' }]);
    overrides = updateOverride(overrides, 'isin:A', { name: 'ASML Holding' });
    expect(overrides).toEqual([{ key: 'isin:A', ticker: 'ASML.AS', name: 'ASML Holding' }]);
    overrides = updateOverride(overrides, 'isin:A', { ticker: null, name: null });
    expect(overrides).toEqual([]);
  });

  it('keeps the order of the others and ignores undefined fields', () => {
    const start: StockInstrumentOverride[] = [
      { key: 'a', ticker: 'A' },
      { key: 'b', ticker: 'B' },
    ];
    const next = updateOverride(start, 'a', { skip: true, name: undefined });
    expect(next).toEqual([
      { key: 'a', ticker: 'A', skip: true },
      { key: 'b', ticker: 'B' },
    ]);
    expect(start[0]).toEqual({ key: 'a', ticker: 'A' });
  });

  it('does not add an override that changes nothing', () => {
    expect(updateOverride([], 'a', { skip: false })).toEqual([]);
  });

  it('knows what changes something and what is skipped', () => {
    expect(hasOverrideContent({ key: 'a' })).toBe(false);
    expect(hasOverrideContent({ key: 'a', skip: false })).toBe(false);
    expect(hasOverrideContent({ key: 'a', currency: 'EUR' })).toBe(true);
    expect(isSkipped([{ key: 'a', skip: true }], 'a')).toBe(true);
    expect(isSkipped([{ key: 'a', skip: true }], 'b')).toBe(false);
  });

  it('never replaces what the user chose with a lookup result', () => {
    const current: StockInstrumentOverride[] = [{ key: 'a', ticker: 'MINE' }, { key: 'x' }];
    const merged = mergeAutoOverrides(current, [
      { key: 'a', ticker: 'THEIRS' },
      { key: 'x', ticker: 'X.DE' },
      { key: 'b', ticker: 'B.DE' },
      { key: 'c' },
    ]);
    expect(merged).toEqual([
      { key: 'a', ticker: 'MINE' },
      { key: 'x' },
      { key: 'x', ticker: 'X.DE' },
      { key: 'b', ticker: 'B.DE' },
    ]);
  });
});

describe('lookups on Yahoo Finance', () => {
  it('asks about an instrument with what the file says', () => {
    expect(
      instrumentQuery(instrument({ isin: 'US0378331005', name: 'Apple', currency: 'USD' }))
    ).toEqual({
      key: 'symbol:AAPL',
      symbol: 'AAPL',
      isin: 'US0378331005',
      name: 'Apple',
      currency: 'USD',
    });
  });

  it('looks up new instruments and those without a symbol only', () => {
    expect(needsLookup(instrument({ status: 'new' }))).toBe(true);
    expect(needsLookup(instrument({ status: 'missingSymbol' }))).toBe(true);
    expect(needsLookup(instrument({ status: 'existing' }))).toBe(false);
    expect(needsLookup(instrument({ status: 'skipped' }))).toBe(false);
  });

  describe('autoOverrideFor', () => {
    const chosen = (symbol: string, currency: string, name = `${symbol} Inc.`) => {
      const best = candidate(symbol, currency, { name });
      return resolution([candidate('OTHER', 'USD'), best], { best });
    };

    it('sets the symbol of an instrument that has none', () => {
      const result = autoOverrideFor(
        instrument({
          key: 'isin:X',
          symbol: null,
          ticker: null,
          status: 'missingSymbol',
          currency: 'EUR',
        }),
        { ...chosen('ASML.AS', 'EUR', ''), key: 'isin:X' }
      );
      expect(result).toEqual({ key: 'isin:X', ticker: 'ASML.AS' });
    });

    it('replaces the file symbol with the listing the backend chose', () => {
      const result = autoOverrideFor(
        instrument({ symbol: 'ASML', ticker: 'ASML', currency: 'EUR', name: 'ASML' }),
        chosen('ASML.AS', 'EUR')
      );
      expect(result).toEqual({ key: 'symbol:AAPL', ticker: 'ASML.AS' });
    });

    it('fills in the name when the file has none', () => {
      const result = autoOverrideFor(
        instrument({ name: null }),
        chosen('AAPL', 'USD', ' Apple Inc. ')
      );
      expect(result).toEqual({ key: 'symbol:AAPL', name: 'Apple Inc.' });
    });

    it('changes nothing when the symbol is already right and the file names it', () => {
      expect(autoOverrideFor(instrument({ name: 'Apple' }), chosen('aapl', 'USD'))).toBeNull();
    });

    it('leaves a listing whose symbol the backend would reject to the user', () => {
      expect(
        autoOverrideFor(
          instrument({ ticker: null, symbol: null, status: 'missingSymbol' }),
          chosen('BAD SYMBOL', 'USD')
        )
      ).toBeNull();
    });

    it('changes nothing when the lookup failed or found nothing', () => {
      expect(autoOverrideFor(instrument(), resolution([], { lookupFailed: true }))).toBeNull();
      expect(autoOverrideFor(instrument(), resolution([]))).toBeNull();
    });
  });

  describe('instrumentCheck', () => {
    it('is pending until there is an answer', () => {
      expect(instrumentCheck(instrument(), undefined)).toEqual({ state: 'pending' });
    });

    it('is verified when the symbol is one of the listings', () => {
      const found = candidate('AAPL', 'USD');
      expect(instrumentCheck(instrument({ ticker: 'aapl' }), resolution([found]))).toEqual({
        state: 'verified',
        candidate: found,
      });
    });

    it('tells apart no answer, no listing and another listing', () => {
      expect(instrumentCheck(instrument(), resolution([], { lookupFailed: true }))).toEqual({
        state: 'failed',
      });
      expect(instrumentCheck(instrument(), resolution([]))).toEqual({ state: 'notFound' });
      expect(instrumentCheck(instrument(), resolution([candidate('AAPL.DE', 'EUR')]))).toEqual({
        state: 'unmatched',
      });
      expect(
        instrumentCheck(instrument({ ticker: null }), resolution([candidate('AAPL', 'USD')]))
      ).toEqual({ state: 'unmatched' });
    });
  });

  it('accepts the tickers the backend accepts', () => {
    expect(isValidTicker('AAPL')).toBe(true);
    expect(isValidTicker(' vwce.de ')).toBe(true);
    expect(isValidTicker('BRK-B')).toBe(true);
    expect(isValidTicker('^GSPC')).toBe(true);
    expect(isValidTicker('EURUSD=X')).toBe(true);
    expect(isValidTicker('')).toBe(false);
    expect(isValidTicker('BAD TICKER')).toBe(false);
    expect(isValidTicker('A'.repeat(21))).toBe(false);
    expect(normalizeTicker(' vwce.de ')).toBe('VWCE.DE');
  });
});

describe('result messages', () => {
  const message = (key: string, line = 2) => ({ line, key, detail: null });

  it('sorts a message into the list it belongs to', () => {
    expect(classifyResultMessage(message('importWizard.row.duplicate'))).toBe('duplicates');
    expect(classifyResultMessage(message('importWizard.row.duplicateExternalId'))).toBe(
      'duplicates'
    );
    expect(classifyResultMessage(message('importWizard.row.notATrade'))).toBe('skipped');
    expect(classifyResultMessage(message('importWizard.row.zeroQuantity'))).toBe('skipped');
    expect(classifyResultMessage(message('importWizard.row.assetClass'))).toBe('skipped');
    expect(classifyResultMessage(message('importWizard.row.instrumentSkipped'))).toBe('skipped');
    expect(classifyResultMessage(message('importWizard.row.dateUnparseable'))).toBe('errors');
    expect(classifyResultMessage(message('validation.currencyInvalid'))).toBe('errors');
    expect(classifyResultMessage(message('something.unknown'))).toBe('errors');
  });

  it('lists a row the review showed as what it was there, whatever its key says', () => {
    const reviewed = rowStatusByLine([
      { line: 3, status: 'duplicate' },
      { line: 4, status: 'skipped' },
      { line: 5, status: 'error' },
      { line: 6, status: 'new' },
    ] as StockPreviewRow[]);
    expect(classifyResultMessage(message('importWizard.row.alreadyThere', 3), reviewed)).toBe(
      'duplicates'
    );
    expect(classifyResultMessage(message('importWizard.row.somethingNew', 4), reviewed)).toBe(
      'skipped'
    );
    expect(classifyResultMessage(message('importWizard.row.notATrade', 5), reviewed)).toBe(
      'errors'
    );
    // a row that was new in the review falls back to its key; a row beyond the review too
    expect(classifyResultMessage(message('importWizard.row.notATrade', 6), reviewed)).toBe(
      'skipped'
    );
    expect(classifyResultMessage(message('importWizard.row.duplicate', 900), reviewed)).toBe(
      'duplicates'
    );
  });

  it('groups the messages and keeps the file order within a group', () => {
    const groups = groupResultMessages([
      message('importWizard.row.notATrade', 3),
      message('importWizard.row.sellExceedsHoldings', 5),
      message('importWizard.row.duplicate', 6),
      message('importWizard.row.notATrade', 7),
    ]);
    expect(groups.skipped.map((m) => m.line)).toEqual([3, 7]);
    expect(groups.errors.map((m) => m.line)).toEqual([5]);
    expect(groups.duplicates.map((m) => m.line)).toEqual([6]);
  });
});

describe('undoableBatches', () => {
  const batch = (id: string, remainingCount: number): StockImportBatch => ({
    id,
    fileName: `${id}.csv`,
    source: 'xtb',
    tradeCount: 5,
    remainingCount,
    createdAt: 1_700_000_000,
  });

  it('hides imports whose trades are all gone', () => {
    expect(undoableBatches([batch('a', 5), batch('b', 0), batch('c', 2)]).map((b) => b.id)).toEqual(
      ['a', 'c']
    );
  });
});

describe('formatDecimalText', () => {
  it('writes a stored decimal in the notation of the UI language', () => {
    expect(formatDecimalText('0.5', 'cs-CZ')).toBe('0,5');
    expect(formatDecimalText('0.5', 'en-US')).toBe('0.5');
    expect(formatDecimalText('1234.5', 'en-US', 2)).toBe('1,234.50');
  });

  it('never shows fewer decimals than the number was written with', () => {
    expect(formatDecimalText('11.7480', 'en-US', 2)).toBe('11.7480');
    expect(formatDecimalText('0.00012345', 'en-US')).toBe('0.00012345');
    expect(formatDecimalText('10', 'en-US')).toBe('10');
  });

  it('shows a dash for a missing value and the text itself for one that is not a number', () => {
    expect(formatDecimalText(null, 'en-US')).toBe('—');
    expect(formatDecimalText('  ', 'en-US')).toBe('—');
    expect(formatDecimalText('n/a', 'en-US')).toBe('n/a');
  });
});

describe('fileStem', () => {
  it('drops the extension and the folder-less path', () => {
    expect(fileStem('history-2024.csv')).toBe('history-2024');
    expect(fileStem('export (1).txt')).toBe('export (1)');
    expect(fileStem('no-extension')).toBe('no-extension');
  });

  it('keeps a name within the length a format may have', () => {
    expect(fileStem(`${'a'.repeat(80)}.csv`)).toHaveLength(60);
  });
});

describe('buildSampleCsv', () => {
  it('starts with a byte order mark so Excel reads the diacritics', () => {
    expect(buildSampleCsv('cs').startsWith('﻿')).toBe(true);
    expect(buildSampleCsv('en').startsWith('﻿')).toBe(true);
  });

  it('is the table a Czech Excel writes in Czech', () => {
    const lines = buildSampleCsv('cs').replace('﻿', '').trim().split('\r\n');
    expect(lines[0]).toBe('Datum;Typ;Symbol;Název;Počet;Cena;Měna');
    expect(lines[1]).toBe('15.01.2024;nákup;AAPL;Apple Inc.;10;180,50;USD');
    expect(lines.some((l) => l.includes(';prodej;'))).toBe(true);
    expect(lines.every((l) => l.split(';').length === 7)).toBe(true);
  });

  it("is Moony's own export format in English", () => {
    const lines = buildSampleCsv('en').replace('﻿', '').trim().split('\r\n');
    expect(lines[0]).toBe('Date,Type,Ticker,Name,Quantity,Price,Currency');
    expect(lines[1]).toBe('2024-01-15,buy,AAPL,Apple Inc.,10,180.50,USD');
    expect(lines.some((l) => l.includes(',sell,'))).toBe(true);
    expect(lines.every((l) => l.split(',').length === 7)).toBe(true);
  });

  it('falls back to English for any other language', () => {
    expect(buildSampleCsv('de')).toBe(buildSampleCsv('en'));
  });

  it('has the same trades in both languages', () => {
    const cells = (csv: string, delimiter: string) =>
      csv
        .replace('﻿', '')
        .trim()
        .split('\r\n')
        .slice(1)
        .map((line) => line.split(delimiter)[2]);
    expect(cells(buildSampleCsv('cs'), ';')).toEqual(cells(buildSampleCsv('en'), ','));
  });
});

describe('EMPTY_MAPPING', () => {
  it('is incomplete and a manual mapping', () => {
    expect(EMPTY_MAPPING.source).toBe('custom');
    expect(isMappingComplete(EMPTY_MAPPING)).toBe(false);
  });
});
