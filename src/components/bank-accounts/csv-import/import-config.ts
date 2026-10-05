/**
 * Pure helpers behind the CSV import dialog: mapping state, the wire config
 * built from it, preview summary and the next-step decisions. No React, no
 * Tauri — everything here is unit-tested (import-config.test.ts).
 */
import { getFormatters } from '@/lib/format';
import { formatSignedAmount } from '@/utils/signed-amount';
import type {
  BankAccount,
  CsvCategoryOverride,
  CsvImportConfigInput,
  CsvImportPreview,
  CsvImportResult,
  CsvPreset,
  CsvPreviewResult,
  CsvRowStatus,
  InsertBankAccount,
} from '@shared/schema';

/** Radix `Select` cannot hold an empty value, so "no column" is a sentinel. */
export const NONE_VALUE = '__none__';
/** Preset select value for "detect the bank from the headers". */
export const AUTO_PRESET = '__auto__';
/** `CsvImportConfigInput.dateFormat` / `decimalSeparator` value for "detect it". */
export const AUTO_VALUE = 'auto';

/** Delay before a changed mapping is sent to `previewCsvImport`. */
export const PREVIEW_DEBOUNCE_MS = 300;

/**
 * Date formats offered next to "Auto". The labels are notation, not prose, so
 * they are not translated. A format the backend detects that is not in this
 * list (`%Y/%m/%d`, ...) is added on the fly by `dateFormatChoices`.
 */
export const DATE_FORMATS: ReadonlyArray<{ value: string; label: string }> = [
  { value: '%Y-%m-%d', label: 'YYYY-MM-DD' },
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

export type DecimalSetting = typeof AUTO_VALUE | ',' | '.';

/** Everything the user can change on step 2 (column names are CSV header texts, '' = none). */
export interface MappingState {
  dateColumn: string;
  amountColumn: string;
  debitColumn: string;
  creditColumn: string;
  balanceColumn: string;
  transactionIdColumn: string;
  descriptionColumns: string[];
  counterpartyColumn: string;
  counterpartyIbanColumn: string;
  currencyColumn: string;
  /** `'auto'` or a chrono format. */
  dateFormat: string;
  decimalSeparator: DecimalSetting;
  /** Apply the detected preset's row filter (`suggestedRowFilter`). */
  rowFilterEnabled: boolean;
}

export const EMPTY_MAPPING: MappingState = {
  dateColumn: '',
  amountColumn: '',
  debitColumn: '',
  creditColumn: '',
  balanceColumn: '',
  transactionIdColumn: '',
  descriptionColumns: [],
  counterpartyColumn: '',
  counterpartyIbanColumn: '',
  currencyColumn: '',
  dateFormat: AUTO_VALUE,
  decimalSeparator: AUTO_VALUE,
  rowFilterEnabled: false,
};

/** Header cells a column can be picked from: not blank, each name once. */
export function uniqueHeaders(headers: readonly string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const header of headers) {
    if (header.trim().length === 0 || seen.has(header)) continue;
    seen.add(header);
    result.push(header);
  }
  return result;
}

/** Maps a `suggestedMappings` role to the `MappingState` field it fills. */
export const ROLE_FIELDS = {
  date: 'dateColumn',
  amount: 'amountColumn',
  debit: 'debitColumn',
  credit: 'creditColumn',
  balance: 'balanceColumn',
  transactionId: 'transactionIdColumn',
  counterparty: 'counterpartyColumn',
  counterparty_iban: 'counterpartyIbanColumn',
  currency: 'currencyColumn',
} as const satisfies Record<string, Exclude<keyof MappingState, 'descriptionColumns'>>;

export type MappingRole = keyof typeof ROLE_FIELDS | 'description';

/** Starting point of step 2: the backend's suggestions (a detected preset's columns come at 1.0). */
export function mappingFromInspection(inspection: CsvPreviewResult): MappingState {
  const suggested = inspection.suggestedMappings ?? {};
  const column = (role: string) => suggested[role]?.[0] ?? '';
  return {
    dateColumn: column('date'),
    amountColumn: column('amount'),
    debitColumn: column('debit'),
    creditColumn: column('credit'),
    balanceColumn: column('balance'),
    transactionIdColumn: column('transactionId'),
    descriptionColumns: [...(inspection.suggestedDescriptionColumns ?? [])],
    counterpartyColumn: column('counterparty'),
    counterpartyIbanColumn: column('counterparty_iban'),
    currencyColumn: column('currency'),
    dateFormat: inspection.dateFormat || AUTO_VALUE,
    decimalSeparator: AUTO_VALUE,
    rowFilterEnabled: inspection.suggestedRowFilter != null,
  };
}

export type ConfidenceLevel = 'high' | 'medium';

/**
 * How sure the backend was about the column now chosen for `role` — only while
 * the user has not picked another column (a badge for a replaced column would
 * vouch for the wrong one). Below 0.7 there is nothing worth showing.
 */
export function suggestionConfidence(
  inspection: Pick<CsvPreviewResult, 'suggestedMappings'>,
  mapping: MappingState,
  role: MappingRole
): { level: ConfidenceLevel; percent: number } | null {
  const suggestion = inspection.suggestedMappings?.[role];
  if (!suggestion) return null;
  const [column, confidence] = suggestion;
  const chosen =
    role === 'description'
      ? mapping.descriptionColumns.includes(column)
      : mapping[ROLE_FIELDS[role]] === column;
  if (!chosen) return null;
  if (confidence >= 0.9) return { level: 'high', percent: Math.round(confidence * 100) };
  if (confidence >= 0.7) return { level: 'medium', percent: Math.round(confidence * 100) };
  return null;
}

/** A date column plus either one amount column or both debit and credit. */
export function isMappingComplete(mapping: MappingState): boolean {
  if (!mapping.dateColumn) return false;
  return (
    Boolean(mapping.amountColumn) || (Boolean(mapping.debitColumn) && Boolean(mapping.creditColumn))
  );
}

/** Debit and credit alone make the amount (credit − debit); a chosen amount column wins. */
export function usesDebitCredit(mapping: MappingState): boolean {
  return !mapping.amountColumn && Boolean(mapping.debitColumn) && Boolean(mapping.creditColumn);
}

/** The inspection fields the import config depends on. */
export type InspectionSettings = Pick<
  CsvPreviewResult,
  'delimiter' | 'headerRow' | 'encoding' | 'suggestedRowFilter'
>;

/**
 * The wire config for `previewCsvImport` / `importCsvTransactions`. Preview
 * and import get the same object so what the user sees is what is written.
 */
export function buildImportConfig(
  mapping: MappingState,
  inspection: InspectionSettings,
  skipRows: number,
  importAnywayLines: readonly number[] = [],
  categoryOverrides: readonly CsvCategoryOverride[] = []
): CsvImportConfigInput {
  const orNull = (value: string) => (value ? value : null);
  // A half-filled pair is not a pair; the backend would reject it.
  const hasPair = Boolean(mapping.debitColumn) && Boolean(mapping.creditColumn);
  return {
    delimiter: inspection.delimiter,
    skipRows,
    headerRow: inspection.headerRow,
    dateColumn: mapping.dateColumn,
    dateFormat: mapping.dateFormat || AUTO_VALUE,
    amountColumn: orNull(mapping.amountColumn),
    debitColumn: hasPair ? mapping.debitColumn : null,
    creditColumn: hasPair ? mapping.creditColumn : null,
    balanceColumn: orNull(mapping.balanceColumn),
    transactionIdColumn: orNull(mapping.transactionIdColumn),
    descriptionColumns: mapping.descriptionColumns.length > 0 ? mapping.descriptionColumns : null,
    counterpartyColumn: orNull(mapping.counterpartyColumn),
    counterpartyIbanColumn: orNull(mapping.counterpartyIbanColumn),
    currencyColumn: orNull(mapping.currencyColumn),
    decimalSeparator: mapping.decimalSeparator,
    rowFilter: mapping.rowFilterEnabled ? inspection.suggestedRowFilter : null,
    skipDuplicates: true,
    importAnywayLines: [...importAnywayLines].sort((a, b) => a - b),
    encoding: inspection.encoding,
    categoryOverrides: [...categoryOverrides].sort((a, b) => a.line - b.line),
  };
}

// ── Date format select ──────────────────────────────────────────────────────

/** Select values: Auto, the known formats, then the current/detected one if it is none of them. */
export function dateFormatChoices(...extra: Array<string | null | undefined>): string[] {
  const known = DATE_FORMATS.map((f) => f.value);
  const choices = [AUTO_VALUE, ...known];
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
 * detected format (or Auto, which detects the same one) is still selected.
 */
export function isDateFormatGuess(
  inspection: Pick<CsvPreviewResult, 'dateFormat' | 'dateFormatAmbiguous'>,
  selectedFormat: string
): boolean {
  return (
    inspection.dateFormatAmbiguous &&
    (selectedFormat === inspection.dateFormat || selectedFormat === AUTO_VALUE)
  );
}

// ── Preview summary and row presentation ────────────────────────────────────

export interface PreviewSummary {
  /** `ok` rows plus the duplicates the user chose to import anyway. */
  willImport: number;
  forcedDuplicates: number;
  skippedDuplicates: number;
  errors: number;
  /** Zero-amount and filtered rows. */
  skipped: number;
  /** Rows that will be written with a category (rules or the user's picks). */
  categorized: number;
  /** Rows that will be written without one. */
  uncategorized: number;
}

/**
 * Counts for the line above the preview table. The backend keeps a forced
 * duplicate's status as `duplicate` (and out of `okCount`), so the rows the
 * user ticked are added back here. Only the first 200 rows are listed — and so
 * tickable — which makes counting them in `rows` exact.
 */
export function summarizePreview(
  preview: CsvImportPreview,
  importAnywayLines: readonly number[]
): PreviewSummary {
  const forced = new Set(importAnywayLines);
  const forcedDuplicates = preview.rows.filter(
    (row) => row.status === 'duplicate' && forced.has(row.line)
  ).length;
  const willImport = preview.okCount + forcedDuplicates;
  const categorized = Math.min(willImport, preview.categorizedCount ?? 0);
  return {
    willImport,
    forcedDuplicates,
    skippedDuplicates: Math.max(0, preview.duplicateCount - forcedDuplicates),
    errors: preview.errorCount,
    skipped: preview.skippedCount,
    categorized,
    uncategorized: willImport - categorized,
  };
}

/**
 * Number with the decimals the text has — at least two when it has any, so
 * "250.5" reads "250.50" — and none when it is whole. Unlike the app's usual
 * currency formatting (CZK without decimals) this never hides cents: the
 * preview and the balance button show exactly what will be stored.
 */
function formatExact(
  value: number,
  text: string,
  currency: string | null | undefined,
  locale: string
): string {
  const written = /\.(\d+)\s*$/.exec(text.trim())?.[1].length ?? 0;
  const digits = written === 0 ? 0 : Math.min(8, Math.max(2, written));
  return getFormatters(locale).money(value, currency ?? '', { decimals: digits });
}

/**
 * "+45 000,50 Kč" / "−1 234,50 Kč" for a preview row's signed amount string;
 * "—" when the row has no readable amount. Credits get an explicit plus so the
 * sign of every row can be checked at a glance.
 */
export function formatPreviewAmount(
  amount: string | null,
  currency: string | null | undefined,
  locale: string
): string {
  if (amount == null) return '—';
  const value = Number(amount);
  if (!Number.isFinite(value)) return amount;
  const text = formatSignedAmount(value, (abs) => formatExact(abs, amount, currency, locale));
  return value > 0 ? `+${text}` : text;
}

export type StatusBadgeVariant = 'gain' | 'loss' | 'default' | 'outline';

/** Badge for a preview row: i18n key (bank_accounts) and the design-system `Badge` variant. */
export function rowStatusPresentation(status: CsvRowStatus): {
  labelKey: string;
  variant: StatusBadgeVariant;
} {
  switch (status) {
    case 'ok':
      return { labelKey: 'csvImport.status.ok', variant: 'gain' };
    case 'error':
      return { labelKey: 'csvImport.status.error', variant: 'loss' };
    case 'duplicate':
      return { labelKey: 'csvImport.status.duplicate', variant: 'default' };
    case 'skipped':
      return { labelKey: 'csvImport.status.skipped', variant: 'outline' };
  }
}

/** Rows shown in the preview before "Show all". */
export const PREVIEW_COLLAPSED_ROWS = 12;

// ── Files ───────────────────────────────────────────────────────────────────

export const SUPPORTED_EXTENSIONS = ['csv', 'txt'] as const;

export function isSupportedCsvPath(path: string): boolean {
  return /\.(csv|txt)$/i.test(path.trim());
}

/** First importable path of a drop (a drop may carry several files and folders). */
export function pickDroppedFile(paths: readonly string[]): string | null {
  return paths.find(isSupportedCsvPath) ?? null;
}

/** Last path segment, for both `/` and `\` separators. */
export function fileNameFromPath(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/** The generic template offered on step 1 ("Download sample CSV"). */
export const SAMPLE_CSV = [
  'Date,Description,Amount,Currency,Counterparty',
  '2025-01-15,Monthly salary,45000.00,CZK,Acme s.r.o.',
  '2025-01-16,Groceries,-1234.50,CZK,Albert',
  '2025-01-18,Coffee,-85.00,CZK,Starbucks',
  '',
].join('\n');

// ── Presets ─────────────────────────────────────────────────────────────────

/** Presets sorted by bank name, diacritics ignored ("Česká spořitelna" among the Cs). */
export function sortPresets(presets: readonly CsvPreset[]): CsvPreset[] {
  return [...presets].sort((a, b) =>
    a.bankName.localeCompare(b.bankName, undefined, { sensitivity: 'base' })
  );
}

export function presetLabel(preset: Pick<CsvPreset, 'bankName' | 'country'>): string {
  return `${preset.bankName} (${preset.country})`;
}

/** The preset seeded for an institution, used to pre-select the bank on step 1. */
export function presetIdForInstitution(
  presets: readonly CsvPreset[],
  institutionId: string | null | undefined
): string | null {
  if (!institutionId) return null;
  return presets.find((p) => p.institutionId === institutionId)?.id ?? null;
}

// ── Next steps after an import ──────────────────────────────────────────────

export type NextStepId = 'show' | 'review' | 'rulePack' | 'balance';

export interface NextStepContext {
  /** The caller can show a date range (`onImported`). */
  canShowImported: boolean;
  /** The caller can filter to uncategorized rows (`onReviewUncategorized`). */
  canReviewUncategorized: boolean;
  /** The account's current (manual) balance; null until it has loaded. */
  accountBalance: string | null;
}

/** Whether the statement's closing balance is a different number than the account's. */
export function balanceDiffers(
  statementBalance: string | null,
  accountBalance: string | null
): boolean {
  if (statementBalance == null || accountBalance == null) return false;
  const statement = Number(statementBalance);
  const account = Number(accountBalance);
  if (!Number.isFinite(statement) || !Number.isFinite(account)) return false;
  return Math.abs(statement - account) >= 0.005;
}

/** The calls to action for the result step, in the order they are shown. */
export function nextStepsFor(result: CsvImportResult, context: NextStepContext): NextStepId[] {
  const steps: NextStepId[] = [];
  if (context.canShowImported && result.importedCount > 0) steps.push('show');
  if (context.canReviewUncategorized && result.uncategorizedCount > 0) steps.push('review');
  if (result.suggestedRulePack) steps.push('rulePack');
  if (balanceDiffers(result.lastBalance, context.accountBalance)) steps.push('balance');
  return steps;
}

/**
 * Account update that changes only the balance. `update_bank_account` writes
 * every field it is given (a missing iban would be cleared), so the rest of the
 * account is sent back unchanged.
 */
export function balanceUpdatePayload(account: BankAccount, balance: string): InsertBankAccount {
  return {
    name: account.name,
    accountType: account.accountType,
    iban: account.iban,
    bban: account.bban,
    currency: account.currency,
    balance,
    institutionId: account.institutionId,
    interestRate: account.interestRate,
    hasZoneDesignation: account.hasZoneDesignation,
    terminationDate: account.terminationDate,
    interestRateValidUntil: account.interestRateValidUntil,
    excludeFromBalance: account.excludeFromBalance,
  };
}

/** A balance as it will be stored ("30 196,12 Kč"), cents included even for CZK. */
export function formatBalance(
  value: string,
  currency: string | null | undefined,
  locale: string
): string {
  const amount = Number(value);
  if (!Number.isFinite(amount)) return value;
  return formatExact(amount, value, currency, locale);
}

/** "Germany" for `de`: the localized region name, or the code when Intl does not know it. */
export function regionName(packId: string, locale: string): string {
  const code = packId.toUpperCase();
  try {
    return new Intl.DisplayNames([locale], { type: 'region' }).of(code) ?? code;
  } catch {
    return code;
  }
}
