/**
 * Pure helpers behind the stock import wizard: sources and their help links,
 * the mapping state and the wire config built from it, the summary of a
 * mapping, preview counts, instrument overrides and the choice of a Yahoo
 * Finance listing, result messages and the sample file. No React, no Tauri —
 * everything here is unit-tested (import-config.test.ts).
 *
 * Columns are 0-based indexes into the header row (the contract in
 * `shared/schema.ts`, section "Stock CSV import"): broker exports have blank
 * and repeated header cells, so a header text cannot address a column.
 */
import { getFormatters } from '@/lib/format';
import type {
  StockColumnSuggestion,
  StockCsvInspection,
  StockCurrencyMode,
  StockDirectionMode,
  StockImportBatch,
  StockImportConfig,
  StockImportInstrument,
  StockImportPreview,
  StockImportTransforms,
  StockInstrumentCandidate,
  StockInstrumentOverride,
  StockInstrumentQuery,
  StockInstrumentResolution,
  StockRowMessage,
  StockTypeValueAction,
  StockTypeValueMapping,
} from '@shared/schema';

/** Radix `Select` cannot hold an empty value, so "no column" is a sentinel. */
export const NONE_VALUE = '__none__';
/** Select value of a setting that follows the detection (decimal separator). */
export const AUTO_VALUE = 'auto';

/** Delay before a changed mapping is sent to `previewStockCsvImport`. */
export const PREVIEW_DEBOUNCE_MS = 300;
/** Rows shown in the review table before "Zobrazit vše". */
export const PREVIEW_COLLAPSED_ROWS = 12;
/**
 * Instruments looked up per call: the backend answers a whole call at once, so
 * small chunks are what makes the progress visible.
 */
export const RESOLVE_CHUNK_SIZE = 6;
/** Longest name of a remembered format. */
export const MAX_FORMAT_NAME_LENGTH = 60;

// ── Sources ─────────────────────────────────────────────────────────────────

/** Entries of the source list on step 1, in the order they are shown. */
export const SOURCE_IDS = ['xtb', 'trading212', 'degiro', 'ibkr', 'moony', 'custom'] as const;
export type SourceId = (typeof SOURCE_IDS)[number];

/** Sources whose export comes from a broker (they have a help page). */
export type BrokerId = 'xtb' | 'trading212' | 'degiro' | 'ibkr';

export const SAVED_FORMAT_PREFIX = 'format:';

export function isSavedFormatSource(source: string | null | undefined): boolean {
  return source != null && source.startsWith(SAVED_FORMAT_PREFIX);
}

export function isSourceId(source: string | null | undefined): source is SourceId {
  return source != null && (SOURCE_IDS as readonly string[]).includes(source);
}

/** The brokers' own pages about exporting a history, by UI language. */
export const SOURCE_HELP_URLS: Record<BrokerId, { cs: string; en: string }> = {
  xtb: {
    cs: 'https://www.xtb.com/cz/help-center',
    en: 'https://www.xtb.com/en/help-center',
  },
  trading212: {
    cs: 'https://helpcentre.trading212.com/hc/en-us/articles/360016898917-Can-I-export-the-trading-data-from-my-account',
    en: 'https://helpcentre.trading212.com/hc/en-us/articles/360016898917-Can-I-export-the-trading-data-from-my-account',
  },
  degiro: {
    cs: 'https://www.degiro.cz/helpdesk/',
    en: 'https://www.degiro.com/uk/helpdesk/',
  },
  ibkr: {
    cs: 'https://www.ibkrguides.com/clientportal/performanceandstatements/flex.htm',
    en: 'https://www.ibkrguides.com/clientportal/performanceandstatements/flex.htm',
  },
};

/** Help page of a source for the UI language; null for sources that have none. */
export function sourceHelpUrl(source: string, language: string): string | null {
  const urls = SOURCE_HELP_URLS[source as BrokerId];
  if (!urls) return null;
  return language === 'cs' ? urls.cs : urls.en;
}

export type SourceMismatch = 'none' | 'other' | 'unknown';

/**
 * Whether the file contradicts the source the user picked on step 1: `other`
 * when its headers belong to another known source or saved format, `unknown`
 * when they belong to none. Nothing is claimed when the user picked nothing,
 * "Jiný broker" (anything goes), or insisted on the source.
 */
export function sourceMismatch(
  chosen: string | null,
  forced: string | null,
  detected: string | null
): SourceMismatch {
  if (chosen == null || chosen === 'custom' || forced === chosen || detected === chosen) {
    return 'none';
  }
  return detected != null ? 'other' : 'unknown';
}

// ── Columns ─────────────────────────────────────────────────────────────────

export interface ColumnOption {
  /** 0-based position in the header row. */
  index: number;
  /** Header text, or a synthetic name for a blank header. */
  label: string;
  /** A few distinct values from the first rows, for telling similar columns apart. */
  example: string;
}

export interface ColumnLabels {
  /** "Sloupec 9" */
  blank: (position: number) => string;
  /** "Sloupec 9 (za „Price“)": a blank header, named after the column before it. */
  blankAfter: (position: number, after: string) => string;
}

const EXAMPLE_MAX_VALUES = 3;
const EXAMPLE_MAX_CELL = 22;

function exampleOf(sampleRows: readonly (readonly string[])[], index: number): string {
  const seen: string[] = [];
  for (const row of sampleRows) {
    const cell = (row[index] ?? '').trim();
    if (cell === '' || seen.includes(cell)) continue;
    seen.push(cell.length > EXAMPLE_MAX_CELL ? `${cell.slice(0, EXAMPLE_MAX_CELL - 1)}…` : cell);
    if (seen.length === EXAMPLE_MAX_VALUES) break;
  }
  return seen.join(', ');
}

/**
 * Every column of the file as a select option. Blank headers get a position
 * name, repeated ones their position, so no two options read the same.
 */
export function columnOptions(
  headers: readonly string[],
  sampleRows: readonly (readonly string[])[],
  labels: ColumnLabels
): ColumnOption[] {
  const seen = new Map<string, number>();
  let previous: string | null = null;
  return headers.map((raw, index) => {
    const header = raw.trim();
    const position = index + 1;
    let label: string;
    if (header === '') {
      label = previous ? labels.blankAfter(position, previous) : labels.blank(position);
    } else {
      label = seen.has(header) ? `${header} (${position})` : header;
      seen.set(header, position);
      previous = header;
    }
    return { index, label, example: exampleOf(sampleRows, index) };
  });
}

/** Label of a column index; "" when there is none. */
export function optionLabel(options: readonly ColumnOption[], index: number | null): string {
  if (index == null) return '';
  return options.find((option) => option.index === index)?.label ?? '';
}

/** Index ↔ Radix select value. */
export function columnToSelectValue(column: number | null): string {
  return column == null ? NONE_VALUE : String(column);
}

export function selectValueToColumn(value: string): number | null {
  if (value === NONE_VALUE || value === '') return null;
  const column = Number(value);
  return Number.isInteger(column) && column >= 0 ? column : null;
}

// ── Mapping state ───────────────────────────────────────────────────────────

export type DecimalSetting = typeof AUTO_VALUE | ',' | '.';

/** Everything the user can change on step 2 (columns are indexes, null = none). */
export interface MappingState {
  /** `StockImportConfig.source`: a built-in source, `custom` or `format:<id>`. */
  source: string;
  dateColumn: number | null;
  /** A chrono format; '' while it is not known yet. */
  dateFormat: string;
  symbolColumn: number | null;
  isinColumn: number | null;
  nameColumn: number | null;
  quantityColumn: number | null;
  priceColumn: number | null;
  currencyMode: StockCurrencyMode;
  currencyColumn: number | null;
  fixedCurrency: string;
  directionMode: StockDirectionMode;
  typeColumn: number | null;
  typeValues: StockTypeValueMapping[];
  /** `'auto'` follows what the inspection detected. */
  decimalSeparator: DecimalSetting;
  externalIdColumn: number | null;
  /** Kept as the adapter set them (XTB comment, IBKR asset class). */
  transforms: StockImportTransforms;
}

export const EMPTY_MAPPING: MappingState = {
  source: 'custom',
  dateColumn: null,
  dateFormat: '',
  symbolColumn: null,
  isinColumn: null,
  nameColumn: null,
  quantityColumn: null,
  priceColumn: null,
  currencyMode: 'instrument',
  currencyColumn: null,
  fixedCurrency: '',
  directionMode: 'quantitySign',
  typeColumn: null,
  typeValues: [],
  decimalSeparator: AUTO_VALUE,
  externalIdColumn: null,
  transforms: {},
};

/** Roles of `StockColumnSuggestion` and the mapping fields they fill. */
export const ROLE_FIELDS = {
  date: 'dateColumn',
  type: 'typeColumn',
  symbol: 'symbolColumn',
  isin: 'isinColumn',
  name: 'nameColumn',
  quantity: 'quantityColumn',
  price: 'priceColumn',
  currency: 'currencyColumn',
  externalId: 'externalIdColumn',
} as const satisfies Record<string, keyof MappingState>;

export type MappingRole = keyof typeof ROLE_FIELDS;

/** The column the inspection suggests for a role (the most confident one), if any. */
export function suggestedColumn(
  suggestions: readonly StockColumnSuggestion[],
  role: MappingRole
): number | null {
  let best: StockColumnSuggestion | null = null;
  for (const suggestion of suggestions) {
    if (suggestion.role === role && (best == null || suggestion.confidence > best.confidence)) {
      best = suggestion;
    }
  }
  return best ? best.column : null;
}

/** The type values of a column as the inspection suggests them (all of them listed). */
export function typeValuesForColumn(
  inspection: Pick<StockCsvInspection, 'columnValues'>,
  column: number | null
): StockTypeValueMapping[] {
  if (column == null) return [];
  const stats = inspection.columnValues.find((entry) => entry.column === column);
  return stats ? stats.values.map((v) => ({ value: v.value, action: v.suggested })) : [];
}

function decimalSettingFor(config: StockImportConfig, detected: string): DecimalSetting {
  if (config.decimalSeparator === detected) return AUTO_VALUE;
  return config.decimalSeparator === ',' ? ',' : '.';
}

/**
 * Starting point of step 2: the ready configuration of a detected source or
 * saved format, else what the headers suggest (the user completes the rest).
 */
export function mappingFromInspection(inspection: StockCsvInspection): MappingState {
  const config = inspection.config;
  if (config) {
    return {
      source: config.source,
      dateColumn: config.dateColumn,
      dateFormat: config.dateFormat,
      symbolColumn: config.symbolColumn ?? null,
      isinColumn: config.isinColumn ?? null,
      nameColumn: config.nameColumn ?? null,
      quantityColumn: config.quantityColumn,
      priceColumn: config.priceColumn,
      currencyMode: config.currencyMode,
      currencyColumn: config.currencyColumn ?? null,
      fixedCurrency: config.fixedCurrency ?? '',
      directionMode: config.directionMode,
      typeColumn: config.typeColumn ?? null,
      typeValues: (config.typeValues ?? []).map((v) => ({ ...v })),
      decimalSeparator: decimalSettingFor(config, inspection.decimalSeparator),
      externalIdColumn: config.externalIdColumn ?? null,
      transforms: { ...(config.transforms ?? {}) },
    };
  }

  const column = (role: MappingRole) => suggestedColumn(inspection.suggestions, role);
  const typeColumn = column('type');
  const currencyColumn = column('currency');
  return {
    ...EMPTY_MAPPING,
    dateColumn: column('date'),
    dateFormat: inspection.dateFormat ?? '',
    symbolColumn: column('symbol'),
    isinColumn: column('isin'),
    nameColumn: column('name'),
    quantityColumn: column('quantity'),
    priceColumn: column('price'),
    currencyMode: currencyColumn != null ? 'column' : 'instrument',
    currencyColumn,
    directionMode: typeColumn != null ? 'typeColumn' : 'quantitySign',
    typeColumn,
    typeValues: typeValuesForColumn(inspection, typeColumn),
    externalIdColumn: column('externalId'),
  };
}

/** The mapping fields that change when another type column is picked. */
export function typeColumnPatch(
  inspection: Pick<StockCsvInspection, 'columnValues'>,
  column: number | null
): Pick<MappingState, 'typeColumn' | 'typeValues'> {
  return { typeColumn: column, typeValues: typeValuesForColumn(inspection, column) };
}

/**
 * The mapping fields that change when the direction is read another way. Going
 * back to a type column picks the suggested one when none was chosen yet; going
 * to the quantity sign keeps the type column, so switching back loses nothing.
 */
export function directionModePatch(
  inspection: Pick<StockCsvInspection, 'suggestions' | 'columnValues'>,
  mapping: Pick<MappingState, 'typeColumn'>,
  mode: StockDirectionMode
): Partial<MappingState> {
  if (mode === 'quantitySign' || mapping.typeColumn != null) return { directionMode: mode };
  return {
    directionMode: mode,
    ...typeColumnPatch(inspection, suggestedColumn(inspection.suggestions, 'type')),
  };
}

export type ConfidenceLevel = 'high' | 'medium';

export interface Confidence {
  level: ConfidenceLevel;
  percent: number;
}

/**
 * How sure the detection was about the column now chosen for `role` — only
 * while the user has not picked another one (a badge for a replaced column
 * would vouch for the wrong one). Below 0.7 there is nothing worth showing.
 */
export function suggestionConfidence(
  inspection: Pick<StockCsvInspection, 'suggestions'>,
  mapping: MappingState,
  role: MappingRole
): Confidence | null {
  const chosen = mapping[ROLE_FIELDS[role]];
  if (chosen == null) return null;
  const suggestion = inspection.suggestions.find((s) => s.role === role && s.column === chosen);
  if (!suggestion) return null;
  if (suggestion.confidence >= 0.9) {
    return { level: 'high', percent: Math.round(suggestion.confidence * 100) };
  }
  if (suggestion.confidence >= 0.7) {
    return { level: 'medium', percent: Math.round(suggestion.confidence * 100) };
  }
  return null;
}

export function isCurrencyCode(code: string): boolean {
  return /^[A-Za-z]{3}$/.test(code.trim());
}

export type MappingField =
  'date' | 'dateFormat' | 'symbol' | 'quantity' | 'price' | 'currency' | 'type';

/** Required settings that are still missing, in the order the form shows them. */
export function incompleteFields(mapping: MappingState): MappingField[] {
  const missing: MappingField[] = [];
  if (mapping.dateColumn == null) missing.push('date');
  if (mapping.dateFormat.trim() === '') missing.push('dateFormat');
  if (mapping.symbolColumn == null && mapping.isinColumn == null) missing.push('symbol');
  if (mapping.quantityColumn == null) missing.push('quantity');
  if (mapping.priceColumn == null) missing.push('price');
  if (mapping.currencyMode === 'column' && mapping.currencyColumn == null) missing.push('currency');
  if (mapping.currencyMode === 'fixed' && !isCurrencyCode(mapping.fixedCurrency)) {
    missing.push('currency');
  }
  if (mapping.directionMode === 'typeColumn' && mapping.typeColumn == null) missing.push('type');
  return missing;
}

/** Everything `validate()` of the Rust config asks for is there. */
export function isMappingComplete(mapping: MappingState): boolean {
  return incompleteFields(mapping).length === 0;
}

/** The inspection fields the import config depends on. */
export type InspectionSettings = Pick<
  StockCsvInspection,
  'delimiter' | 'headerRow' | 'encoding' | 'decimalSeparator'
>;

/** What the user adds to the mapping while reviewing a file. */
export interface ReviewChoices {
  /** Data rows skipped after the header. */
  skipRows?: number;
  /** Duplicate rows (1-based file lines) to import anyway. */
  importAnywayLines?: readonly number[];
  instrumentOverrides?: readonly StockInstrumentOverride[];
}

/**
 * The wire config for `previewStockCsvImport` / `importStockCsv`, or null while
 * the mapping is incomplete. Preview and import get the same object, so what
 * the user sees is what is written.
 */
export function buildStockImportConfig(
  mapping: MappingState,
  inspection: InspectionSettings,
  choices: ReviewChoices = {}
): StockImportConfig | null {
  if (!isMappingComplete(mapping)) return null;
  const byColumn = mapping.directionMode === 'typeColumn';
  return {
    source: mapping.source,
    delimiter: inspection.delimiter,
    encoding: inspection.encoding,
    headerRow: inspection.headerRow,
    skipRows: choices.skipRows ?? 0,
    dateColumn: mapping.dateColumn as number,
    dateFormat: mapping.dateFormat,
    symbolColumn: mapping.symbolColumn,
    isinColumn: mapping.isinColumn,
    nameColumn: mapping.nameColumn,
    quantityColumn: mapping.quantityColumn as number,
    priceColumn: mapping.priceColumn as number,
    currencyMode: mapping.currencyMode,
    currencyColumn: mapping.currencyMode === 'column' ? mapping.currencyColumn : null,
    fixedCurrency:
      mapping.currencyMode === 'fixed' ? mapping.fixedCurrency.trim().toUpperCase() : null,
    directionMode: mapping.directionMode,
    typeColumn: byColumn ? mapping.typeColumn : null,
    typeValues: byColumn ? mapping.typeValues : [],
    decimalSeparator:
      mapping.decimalSeparator === AUTO_VALUE
        ? inspection.decimalSeparator === ','
          ? ','
          : '.'
        : mapping.decimalSeparator,
    externalIdColumn: mapping.externalIdColumn,
    transforms: mapping.transforms,
    instrumentOverrides: (choices.instrumentOverrides ?? []).filter(hasOverrideContent),
    importAnywayLines: [...(choices.importAnywayLines ?? [])].sort((a, b) => a - b),
  };
}

/**
 * The config a remembered format keeps: the mapping alone. What belongs to one
 * file (skipped rows, instrument choices, duplicates to import) is left out.
 */
export function formatConfig(config: StockImportConfig): StockImportConfig {
  return { ...config, skipRows: 0, instrumentOverrides: [], importAnywayLines: [] };
}

// ── Type values ─────────────────────────────────────────────────────────────

/** Case, diacritics and surrounding spaces do not tell two spellings of a type word apart. */
function normalizeTypeValue(value: string): string {
  return value
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .trim()
    .toLowerCase();
}

/**
 * The meaning of a type value; a value that is not listed is skipped. The
 * value as it is written in the file wins, a listing of the same word in
 * another spelling ("Buy" for "buy", "Nákup" for "nakup") is the fallback.
 */
export function typeValueAction(
  typeValues: readonly StockTypeValueMapping[],
  value: string
): StockTypeValueAction {
  const exact = typeValues.find((entry) => entry.value === value);
  if (exact) return exact.action;
  const wanted = normalizeTypeValue(value);
  return typeValues.find((entry) => normalizeTypeValue(entry.value) === wanted)?.action ?? 'skip';
}

/** `typeValues` with one value set to a meaning. */
export function setTypeValueAction(
  typeValues: readonly StockTypeValueMapping[],
  value: string,
  action: StockTypeValueAction
): StockTypeValueMapping[] {
  const exists = typeValues.some((entry) => entry.value === value);
  return exists
    ? typeValues.map((entry) => (entry.value === value ? { value, action } : entry))
    : [...typeValues, { value, action }];
}

/** Some value is read as a buy or a sell: without one nothing would be imported. */
export function hasTradeValue(typeValues: readonly StockTypeValueMapping[]): boolean {
  return typeValues.some((entry) => entry.action !== 'skip');
}

// ── Mapping summary (known sources) ─────────────────────────────────────────

export type SummaryRole =
  | 'date'
  | 'direction'
  | 'symbol'
  | 'isin'
  | 'name'
  | 'quantity'
  | 'price'
  | 'quantityPrice'
  | 'currency'
  | 'externalId';

export interface MappingSummaryItem {
  role: SummaryRole;
  /**
   * `column`: the value is a header; `sign`: direction from the sign of the
   * quantity; `fixed`: the value is a currency code; `instrument`: currency
   * from the listing; `comment`: quantity and price are read from a comment.
   */
  kind: 'column' | 'sign' | 'fixed' | 'instrument' | 'comment';
  value: string;
}

/** What a mapping reads from where, for the compact summary of a known source. */
export function summarizeMapping(
  mapping: MappingState,
  options: readonly ColumnOption[]
): MappingSummaryItem[] {
  const label = (column: number | null) => optionLabel(options, column);
  const items: MappingSummaryItem[] = [];
  const column = (role: SummaryRole, index: number | null) => {
    if (index != null) items.push({ role, kind: 'column', value: label(index) });
  };

  column('date', mapping.dateColumn);
  if (mapping.directionMode === 'typeColumn') column('direction', mapping.typeColumn);
  else items.push({ role: 'direction', kind: 'sign', value: '' });
  column('symbol', mapping.symbolColumn);
  column('isin', mapping.isinColumn);
  column('name', mapping.nameColumn);
  if (mapping.transforms.xtbComment && mapping.quantityColumn === mapping.priceColumn) {
    items.push({ role: 'quantityPrice', kind: 'comment', value: label(mapping.quantityColumn) });
  } else {
    column('quantity', mapping.quantityColumn);
    column('price', mapping.priceColumn);
  }
  if (mapping.currencyMode === 'column') column('currency', mapping.currencyColumn);
  else if (mapping.currencyMode === 'fixed') {
    items.push({ role: 'currency', kind: 'fixed', value: mapping.fixedCurrency.toUpperCase() });
  } else items.push({ role: 'currency', kind: 'instrument', value: '' });
  column('externalId', mapping.externalIdColumn);
  return items;
}

/**
 * The values of the type column in the file, grouped by what they mean
 * ("Market buy, Limit buy → Nákup"). Values the file does not contain are left
 * out; those it has but the mapping does not list are skipped.
 */
export function groupTypeValues(
  mapping: MappingState,
  inspection: Pick<StockCsvInspection, 'columnValues'>
): Record<StockTypeValueAction, string[]> {
  const groups: Record<StockTypeValueAction, string[]> = { buy: [], sell: [], skip: [] };
  if (mapping.directionMode !== 'typeColumn' || mapping.typeColumn == null) return groups;
  const stats = inspection.columnValues.find((entry) => entry.column === mapping.typeColumn);
  const values = stats ? stats.values.map((v) => v.value) : mapping.typeValues.map((v) => v.value);
  for (const value of values) groups[typeValueAction(mapping.typeValues, value)].push(value);
  return groups;
}

/** Roles of every mapped column, for highlighting the raw preview. */
export function mappedColumns(mapping: MappingState): Map<number, MappingRole[]> {
  const result = new Map<number, MappingRole[]>();
  const add = (column: number | null, role: MappingRole) => {
    if (column == null) return;
    result.set(column, [...(result.get(column) ?? []), role]);
  };
  add(mapping.dateColumn, 'date');
  if (mapping.directionMode === 'typeColumn') add(mapping.typeColumn, 'type');
  add(mapping.symbolColumn, 'symbol');
  add(mapping.isinColumn, 'isin');
  add(mapping.nameColumn, 'name');
  add(mapping.quantityColumn, 'quantity');
  add(mapping.priceColumn, 'price');
  if (mapping.currencyMode === 'column') add(mapping.currencyColumn, 'currency');
  add(mapping.externalIdColumn, 'externalId');
  return result;
}

// ── Date format select ──────────────────────────────────────────────────────

/**
 * Date formats offered in the select. The labels are notation, not prose, so
 * they are not translated. A detected format that is not in this list is added
 * on the fly by `dateFormatChoices`.
 */
export const DATE_FORMATS: ReadonlyArray<{ value: string; label: string }> = [
  { value: '%Y-%m-%d', label: 'YYYY-MM-DD' },
  { value: '%Y%m%d', label: 'YYYYMMDD' },
  { value: '%d.%m.%Y', label: 'DD.MM.YYYY' },
  { value: '%d/%m/%Y', label: 'DD/MM/YYYY' },
  { value: '%m/%d/%Y', label: 'MM/DD/YYYY' },
  { value: '%d-%m-%Y', label: 'DD-MM-YYYY' },
  { value: '%d.%m.%y', label: 'DD.MM.YY' },
  // Two-digit years can be first or last; the backend flags such a file as a
  // guess, so every reading has to be selectable.
  { value: '%d-%m-%y', label: 'DD-MM-YY' },
  { value: '%d/%m/%y', label: 'DD/MM/YY' },
  { value: '%m/%d/%y', label: 'MM/DD/YY' },
  { value: '%y-%m-%d', label: 'YY-MM-DD' },
];

/** The known formats, then the current/detected ones if they are none of them. */
export function dateFormatChoices(...extra: Array<string | null | undefined>): string[] {
  const choices = DATE_FORMATS.map((f) => f.value);
  for (const value of extra) {
    if (value && !choices.includes(value)) choices.push(value);
  }
  return choices;
}

/** Notation of a chrono format (`DD.MM.YYYY`); unknown formats show their raw pattern. */
export function dateFormatLabel(value: string): string {
  return DATE_FORMATS.find((f) => f.value === value)?.label ?? value;
}

/**
 * The day/month order is only a guess while the user has not touched it: the
 * detected format is still selected on the column it was detected for.
 */
export function isDateFormatGuess(
  inspection: Pick<StockCsvInspection, 'dateFormat' | 'dateFormatAmbiguous' | 'suggestions'>,
  mapping: Pick<MappingState, 'dateFormat' | 'dateColumn'>
): boolean {
  if (!inspection.dateFormatAmbiguous) return false;
  return (
    mapping.dateFormat === inspection.dateFormat &&
    mapping.dateColumn === suggestedColumn(inspection.suggestions, 'date')
  );
}

// ── Preview ─────────────────────────────────────────────────────────────────

export interface PreviewSummary {
  /** Rows that will be written, forced duplicates included. */
  willImport: number;
  duplicates: number;
  skipped: number;
  errors: number;
  total: number;
  /** Duplicates the user chose to import anyway. */
  forcedDuplicates: number;
}

/**
 * Counts for the strip above the review table. Whether the backend turns a
 * forced duplicate into a `new` row or keeps it a `duplicate`, the user's ticks
 * end up counted once: the ones still reported as duplicates are moved over
 * here. Only the first 200 rows are listed — and so tickable — which makes
 * counting them in `rows` exact.
 */
export function summarizePreview(
  preview: StockImportPreview,
  importAnywayLines: readonly number[]
): PreviewSummary {
  const forced = new Set(importAnywayLines);
  const stillDuplicate = preview.rows.filter(
    (row) => row.status === 'duplicate' && forced.has(row.line)
  ).length;
  const { counts } = preview;
  return {
    willImport: counts.willImport + stillDuplicate,
    duplicates: Math.max(0, counts.duplicates - stillDuplicate),
    skipped: counts.skipped,
    errors: counts.errors,
    total: counts.total,
    forcedDuplicates: stillDuplicate,
  };
}

/** Instruments that must get a symbol (or be skipped) before anything is written. */
export function missingSymbolCount(preview: Pick<StockImportPreview, 'instruments'>): number {
  return preview.instruments.filter((i) => i.status === 'missingSymbol').length;
}

// ── Instruments: overrides and Yahoo Finance listings ───────────────────────

type OverridePatch = Partial<
  Pick<StockInstrumentOverride, 'ticker' | 'name' | 'currency' | 'skip'>
>;

/** An override that changes something; an empty one is dropped. */
export function hasOverrideContent(override: StockInstrumentOverride): boolean {
  return Boolean(override.ticker || override.name || override.currency || override.skip);
}

/**
 * `overrides` with the user's change to one instrument merged in, in place;
 * an override left with nothing in it is removed.
 */
export function updateOverride(
  overrides: readonly StockInstrumentOverride[],
  key: string,
  patch: OverridePatch
): StockInstrumentOverride[] {
  const defined = Object.fromEntries(
    Object.entries(patch).filter(([, value]) => value !== undefined)
  ) as OverridePatch;
  const existing = overrides.find((o) => o.key === key);
  const next: StockInstrumentOverride = { ...existing, key, ...defined };
  if (!existing) return hasOverrideContent(next) ? [...overrides, next] : [...overrides];
  return overrides.flatMap((o) => {
    if (o.key !== key) return [o];
    return hasOverrideContent(next) ? [next] : [];
  });
}

/** An instrument's skip flag, as set in the overrides. */
export function isSkipped(overrides: readonly StockInstrumentOverride[], key: string): boolean {
  return overrides.find((o) => o.key === key)?.skip === true;
}

function sameSymbol(a: string | null | undefined, b: string | null | undefined): boolean {
  return a != null && b != null && a.trim().toUpperCase() === b.trim().toUpperCase();
}

/** What to look up on Yahoo Finance for one instrument of the preview. */
export function instrumentQuery(instrument: StockImportInstrument): StockInstrumentQuery {
  return {
    key: instrument.key,
    symbol: instrument.symbol,
    isin: instrument.isin,
    name: instrument.name,
    currency: instrument.currency,
  };
}

/** Instruments whose symbol still has to be confirmed on Yahoo Finance. */
export function needsLookup(instrument: StockImportInstrument): boolean {
  return instrument.status === 'new' || instrument.status === 'missingSymbol';
}

/**
 * The listing to use for an instrument. The trade currency decides before the
 * symbol does: a broker's `ASML` in euro is the Amsterdam listing, not the
 * Nasdaq one that happens to share the symbol. Without a trade currency the
 * exact symbol wins, then the backend's pick, then the first result.
 */
export function chooseCandidate(
  resolution: Pick<StockInstrumentResolution, 'candidates' | 'best'>,
  tradeCurrency: string | null,
  symbol: string | null
): StockInstrumentCandidate | null {
  const { candidates, best } = resolution;
  if (candidates.length === 0) return best;
  if (tradeCurrency) {
    const inCurrency = candidates.filter((c) => sameSymbol(c.currency, tradeCurrency));
    const exact = inCurrency.find((c) => sameSymbol(c.symbol, symbol));
    if (exact) return exact;
    if (inCurrency.length > 0) return inCurrency[0];
  }
  return candidates.find((c) => sameSymbol(c.symbol, symbol)) ?? best ?? candidates[0];
}

/**
 * The override a found listing implies: its symbol when that is not the one the
 * trades would be stored under, its name when the file has none. Null when
 * there is nothing to change.
 */
export function autoOverrideFor(
  instrument: StockImportInstrument,
  resolution: StockInstrumentResolution
): StockInstrumentOverride | null {
  if (resolution.lookupFailed) return null;
  const candidate = chooseCandidate(resolution, instrument.currency, instrument.symbol);
  // A symbol the backend would reject makes the whole preview fail: leave it to the user.
  if (!candidate || !isValidTicker(candidate.symbol)) return null;
  const override: StockInstrumentOverride = { key: instrument.key };
  if (!sameSymbol(instrument.ticker, candidate.symbol)) {
    override.ticker = candidate.symbol.trim().toUpperCase();
  }
  if (!instrument.name?.trim() && candidate.name.trim()) override.name = candidate.name.trim();
  return hasOverrideContent(override) ? override : null;
}

/**
 * Adds the automatic overrides to the user's. What the user already chose for
 * an instrument is never replaced by a lookup result that arrives later.
 */
export function mergeAutoOverrides(
  current: readonly StockInstrumentOverride[],
  automatic: readonly StockInstrumentOverride[]
): StockInstrumentOverride[] {
  const taken = new Set(current.filter(hasOverrideContent).map((o) => o.key));
  const added = automatic.filter((o) => hasOverrideContent(o) && !taken.has(o.key));
  return added.length === 0 ? [...current] : [...current, ...added];
}

/** How far an instrument's symbol is confirmed. */
export type InstrumentCheck =
  /** Not looked up (yet). */
  | { state: 'pending' }
  /** The symbol is a listing Yahoo Finance knows. */
  | { state: 'verified'; candidate: StockInstrumentCandidate }
  /** Yahoo Finance did not answer (offline, rate limit) or the check was skipped. */
  | { state: 'failed' }
  /** Yahoo Finance answered and knows nothing under this name. */
  | { state: 'notFound' }
  /** Yahoo Finance knows listings, but not the symbol the trades use. */
  | { state: 'unmatched' };

export function instrumentCheck(
  instrument: Pick<StockImportInstrument, 'ticker'>,
  resolution: StockInstrumentResolution | undefined
): InstrumentCheck {
  if (!resolution) return { state: 'pending' };
  if (resolution.lookupFailed) return { state: 'failed' };
  if (resolution.candidates.length === 0) return { state: 'notFound' };
  const candidate = resolution.candidates.find((c) => sameSymbol(c.symbol, instrument.ticker));
  return candidate ? { state: 'verified', candidate } : { state: 'unmatched' };
}

/** A ticker the backend accepts: 1–20 letters, digits and `. - ^ =`. */
export function isValidTicker(text: string): boolean {
  return /^[A-Za-z0-9.\-^=]{1,20}$/.test(text.trim());
}

export function normalizeTicker(text: string): string {
  return text.trim().toUpperCase();
}

// ── Messages of the result ──────────────────────────────────────────────────

export type ResultMessageGroup = 'duplicates' | 'skipped' | 'errors';

const SKIPPED_KEYS: ReadonlySet<string> = new Set([
  'importWizard.row.notATrade',
  'importWizard.row.zeroQuantity',
  'importWizard.row.assetClass',
  'importWizard.row.instrumentSkipped',
]);

const DUPLICATE_KEY_PREFIX = 'importWizard.row.duplicate';

/** Which list of the result step a row message belongs to. */
export function classifyResultMessage(message: Pick<StockRowMessage, 'key'>): ResultMessageGroup {
  if (message.key.startsWith(DUPLICATE_KEY_PREFIX)) return 'duplicates';
  if (SKIPPED_KEYS.has(message.key)) return 'skipped';
  return 'errors';
}

export function groupResultMessages(
  messages: readonly StockRowMessage[]
): Record<ResultMessageGroup, StockRowMessage[]> {
  const groups: Record<ResultMessageGroup, StockRowMessage[]> = {
    duplicates: [],
    skipped: [],
    errors: [],
  };
  for (const message of messages) groups[classifyResultMessage(message)].push(message);
  return groups;
}

// ── Recent imports ──────────────────────────────────────────────────────────

/** Imports that can still be undone: some of their trades are still there. */
export function undoableBatches(batches: readonly StockImportBatch[]): StockImportBatch[] {
  return batches.filter((batch) => batch.remainingCount > 0);
}

// ── Numbers, names, files ───────────────────────────────────────────────────

/**
 * A decimal as the backend stores it ("0.5", "180.50") in the UI language's
 * notation, with at least `minDecimals` digits and never fewer than it was
 * written with (up to 8): the review shows what will be stored.
 */
export function formatDecimalText(
  text: string | null | undefined,
  locale: string,
  minDecimals = 0
): string {
  if (text == null || text.trim() === '') return '—';
  const value = Number(text);
  if (!Number.isFinite(value)) return text;
  const written = /\.(\d+)\s*$/.exec(text.trim())?.[1].length ?? 0;
  const digits = Math.min(8, Math.max(minDecimals, written));
  return getFormatters(locale).number(value, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
}

/** The name of a file without its extension, as a starting name for a format. */
export function fileStem(fileName: string): string {
  const stem = fileName.replace(/\.[^./\\]+$/, '').trim();
  return stem.length > MAX_FORMAT_NAME_LENGTH ? stem.slice(0, MAX_FORMAT_NAME_LENGTH).trim() : stem;
}

/** Excel opens a UTF-8 file with this mark in front as UTF-8 (diacritics survive). */
const BYTE_ORDER_MARK = '﻿';

/**
 * The Moony table filled with a few example trades, as the file offered for
 * download. Czech: the way a Czech Excel writes it (semicolon, decimal comma,
 * day-first dates, Czech headers and type words); English: Moony's own export
 * format. Both are read back without any mapping.
 */
export function buildSampleCsv(language: string): string {
  const rows =
    language === 'cs'
      ? [
          'Datum;Typ;Symbol;Název;Počet;Cena;Měna',
          '15.01.2024;nákup;AAPL;Apple Inc.;10;180,50;USD',
          '20.02.2024;nákup;VWCE.DE;Vanguard FTSE All-World UCITS ETF;5;108,30;EUR',
          '12.03.2024;nákup;AAPL;Apple Inc.;5;172,10;USD',
          '03.06.2024;prodej;AAPL;Apple Inc.;3;194,75;USD',
          '17.09.2024;nákup;CEZ.PR;ČEZ;20;1020,00;CZK',
        ]
      : [
          'Date,Type,Ticker,Name,Quantity,Price,Currency',
          '2024-01-15,buy,AAPL,Apple Inc.,10,180.50,USD',
          '2024-02-20,buy,VWCE.DE,Vanguard FTSE All-World UCITS ETF,5,108.30,EUR',
          '2024-03-12,buy,AAPL,Apple Inc.,5,172.10,USD',
          '2024-06-03,sell,AAPL,Apple Inc.,3,194.75,USD',
          '2024-09-17,buy,CEZ.PR,ČEZ,20,1020.00,CZK',
        ];
  return `${BYTE_ORDER_MARK}${rows.join('\r\n')}\r\n`;
}
