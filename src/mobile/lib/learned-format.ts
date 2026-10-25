/**
 * Learned CSV formats on the phone (spec §8.6, rung 4), pure: a layout the person confirmed once is
 * remembered under its header signature, and next month's file of the same bank goes straight to
 * the preview. The store is `learnedFormatsApi` (Rust `services/csv_import/learned.rs`); this file
 * holds what the import flow decides around it — how a stored mapping is read against the file in
 * hand, what a new format is called, which account it preselects and when an import teaches or
 * marks one used — and what Nastavení → Import says about a stored format (`learnedFormatSub`).
 * Nothing here touches the network, a file or a React hook.
 */
import type {
  CsvPreviewResult,
  InsertLearnedCsvFormat,
  LearnedCsvFormat,
  LearnedCsvMapping,
} from '@shared/schema';
import {
  AUTO_VALUE,
  EMPTY_MAPPING,
  isMappingComplete,
  uniqueHeaders,
  type DecimalSetting,
  type MappingState,
} from '@/components/bank-accounts/csv-import/import-config';
import { isOpenAccount, type ImportAccount } from '@/mobile/lib/import-flow';

/** Query key of `learnedFormatsApi.list()`: the CSV layouts the person confirmed once. */
export const LEARNED_FORMATS_KEY = ['learned-csv-formats'] as const;

/** The longest name the store takes (characters). */
const MAX_NAME_CHARS = 60;

/**
 * Whitespace as Rust's `char::is_whitespace` has it (the Unicode `White_Space` property). A
 * JavaScript `\s` is not the same set: it counts U+FEFF and leaves out U+0085.
 */
const WHITESPACE =
  '\\t-\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000';
const EDGES = new RegExp(`^[${WHITESPACE}]+|[${WHITESPACE}]+$`, 'g');
const RUNS = new RegExp(`[${WHITESPACE}]+`, 'g');
const LEADING_BOM = /^\uFEFF+/;

/**
 * A header cell as the layout signature compares it, equal to `normalise_cell` in
 * `learned.rs`: trimmed, a BOM at the start dropped, whitespace runs one space, lowercase. An
 * empty cell stays empty. A stored column is matched to the file's own header with it.
 */
export function normalizeHeader(cell: string): string {
  return cell
    .replace(EDGES, '')
    .replace(LEADING_BOM, '')
    .replace(RUNS, ' ')
    .replace(EDGES, '')
    .toLowerCase();
}

/**
 * The mapping a confirmed recognition goes into the store as: no empty column names (`null`),
 * the description columns in their order, `auto` for a date format or decimal separator left to
 * detect. The preset row filter is not part of a layout.
 */
export function learnedMappingFrom(mapping: MappingState): LearnedCsvMapping {
  const column = (name: string) => (name === '' ? null : name);
  return {
    dateColumn: mapping.dateColumn,
    dateFormat: mapping.dateFormat || AUTO_VALUE,
    amountColumn: column(mapping.amountColumn),
    debitColumn: column(mapping.debitColumn),
    creditColumn: column(mapping.creditColumn),
    balanceColumn: column(mapping.balanceColumn),
    transactionIdColumn: column(mapping.transactionIdColumn),
    descriptionColumns: [...mapping.descriptionColumns],
    counterpartyColumn: column(mapping.counterpartyColumn),
    counterpartyIbanColumn: column(mapping.counterpartyIbanColumn),
    counterpartyBankCodeColumn: column(mapping.counterpartyBankCodeColumn),
    counterpartyCountryColumn: column(mapping.counterpartyCountryColumn),
    currencyColumn: column(mapping.currencyColumn),
    decimalSeparator: mapping.decimalSeparator,
  };
}

const decimalSetting = (value: string): DecimalSetting =>
  value === ',' || value === '.' ? value : AUTO_VALUE;

/**
 * A stored format read against the file in hand: every stored column is rewritten to the file's own
 * header cell, matched by `normalizeHeader` (the one written exactly as stored first, so a file
 * that names "Částka" twice in two cases keeps the cell it was learned from). The import never
 * runs on a partial mapping: `null` when the date, or the amount and not a whole debit and credit
 * pair, cannot be found — the flow then recognizes the file from its content. An optional column
 * that cannot be found is left out (`''`), as is the half of a pair next to an amount.
 */
export function mappingForFile(
  format: Pick<LearnedCsvFormat, 'mapping'>,
  inspection: Pick<CsvPreviewResult, 'headers'>
): MappingState | null {
  const stored = format.mapping;
  const cells = uniqueHeaders(inspection.headers);
  const resolve = (name: string | null): string => {
    if (name === null) return '';
    const wanted = normalizeHeader(name);
    if (wanted === '') return '';
    return (
      cells.find((cell) => cell === name) ??
      cells.find((cell) => normalizeHeader(cell) === wanted) ??
      ''
    );
  };

  const debit = resolve(stored.debitColumn);
  const credit = resolve(stored.creditColumn);
  const pair = debit !== '' && credit !== '';
  const description = stored.descriptionColumns.map(resolve).filter((name) => name !== '');
  const dateFormat = stored.dateFormat.trim();

  const mapping: MappingState = {
    ...EMPTY_MAPPING,
    dateColumn: resolve(stored.dateColumn),
    dateFormat:
      dateFormat === '' || dateFormat.toLowerCase() === AUTO_VALUE ? AUTO_VALUE : stored.dateFormat,
    amountColumn: resolve(stored.amountColumn),
    debitColumn: pair ? debit : '',
    creditColumn: pair ? credit : '',
    balanceColumn: resolve(stored.balanceColumn),
    transactionIdColumn: resolve(stored.transactionIdColumn),
    descriptionColumns: [...new Set(description)],
    counterpartyColumn: resolve(stored.counterpartyColumn),
    counterpartyIbanColumn: resolve(stored.counterpartyIbanColumn),
    counterpartyBankCodeColumn: resolve(stored.counterpartyBankCodeColumn),
    counterpartyCountryColumn: resolve(stored.counterpartyCountryColumn),
    currencyColumn: resolve(stored.currencyColumn),
    decimalSeparator: decimalSetting(stored.decimalSeparator),
    rowFilterEnabled: false,
  };
  return isMappingComplete(mapping) ? mapping : null;
}

/**
 * The name a new format gets: the account's institution, else the account's name, else the file's
 * name without its extension; at most 60 characters (the store counts characters, not UTF-16
 * units). A person's own name for a layout is never replaced by this: the flow keeps it.
 */
export function defaultFormatName(
  account: { name: string; institution?: { name: string } | null } | undefined,
  fileName: string
): string {
  const clip = (text: string) => Array.from(text.trim()).slice(0, MAX_NAME_CHARS).join('').trim();
  const stem = fileName.replace(/\.[^./\\]*$/, '');
  return (
    [account?.institution?.name, account?.name, stem, fileName]
      .map((candidate) => clip(candidate ?? ''))
      .find((candidate) => candidate !== '') ?? ''
  );
}

/**
 * The account a known layout's import goes to without asking: the one the entry point named, else
 * the one the format was learned for while it still exists and is open. Anything else is left to
 * `chooseAccount` (the only open account, or the account list).
 */
export function learnedPreselect(input: {
  format: LearnedCsvFormat | null;
  /** The account the entry point named (or a new one made for the file). */
  requested?: string;
  accounts: readonly ImportAccount[];
  todayDay: number;
}): string | undefined {
  if (input.requested) return input.requested;
  const remembered = input.format?.accountId;
  if (!remembered) return undefined;
  const account = input.accounts.find((candidate) => candidate.id === remembered);
  return account && isOpenAccount(account, input.todayDay) ? account.id : undefined;
}

/** How the CSV in the sheet is read: a bank preset, a format the person learned, the recognition. */
export type CsvSource = 'preset' | 'learned' | 'recognized';

/** What a successful import teaches the store. */
export type Learning =
  | { kind: 'save'; insert: InsertLearnedCsvFormat }
  | { kind: 'markUsed'; id: string }
  | { kind: 'none' };

/**
 * What the store is told after an import succeeded: a layout that was recognized (also one the
 * person changed from a learned format) is saved, replacing the earlier format of its header
 * signature; a learned format that read the file is marked used; a preset teaches nothing.
 */
export function learningAfterImport(input: {
  source: CsvSource;
  learned: LearnedCsvFormat | null;
  /** What `learnedFormatInsert` made of the recognized layout; `null` when there is none. */
  insert: InsertLearnedCsvFormat | null;
}): Learning {
  if (input.source === 'recognized' && input.insert) return { kind: 'save', insert: input.insert };
  if (input.source === 'learned' && input.learned) {
    return { kind: 'markUsed', id: input.learned.id };
  }
  return { kind: 'none' };
}

/** What `applyLearning` needs of `learnedFormatsApi`. */
export interface LearningStore {
  save: (format: InsertLearnedCsvFormat) => Promise<LearnedCsvFormat>;
  markUsed: (id: string) => Promise<void>;
}

/**
 * Tells the store what an import taught it. Best effort, by design: the import already succeeded,
 * so a refusal (a header row that is data, `validation.learnedFormatHeaderless`, nothing is
 * stored then) or an error means "not learned" and never reaches the person. Resolves with the
 * format that was saved — the only thing the done step reports — and `null` for everything else.
 */
export async function applyLearning(
  learning: Learning,
  store: LearningStore
): Promise<LearnedCsvFormat | null> {
  try {
    if (learning.kind === 'save') return await store.save(learning.insert);
    if (learning.kind === 'markUsed') await store.markUsed(learning.id);
  } catch {
    // Not learned: nothing to tell the person about an import that worked
  }
  return null;
}

/**
 * The layout to remember: the file's header cells, delimiter, encoding and header row, the mapping
 * the person accepted and the account the import wrote to. Only the layout — no row of the file.
 */
export function learnedFormatInsert(input: {
  name: string;
  accountId: string | null;
  csv: Pick<CsvPreviewResult, 'headers' | 'delimiter' | 'encoding' | 'headerRow'>;
  mapping: MappingState;
}): InsertLearnedCsvFormat {
  const { csv } = input;
  return {
    name: input.name,
    headers: [...csv.headers],
    delimiter: csv.delimiter,
    encoding: csv.encoding,
    headerRow: csv.headerRow,
    mapping: learnedMappingFrom(input.mapping),
    accountId: input.accountId,
  };
}

/**
 * What a learned format's row and sheet say about it besides its name: the account it was learned
 * for — its name with the institution ("Běžný účet · UniCredit"), `null` when it has none or the
 * account has since been deleted — and the two dates (unix seconds, as stored; the caller writes
 * them in the person's language).
 */
export function learnedFormatSub(
  format: Pick<LearnedCsvFormat, 'accountId' | 'createdAt' | 'lastUsedAt'>,
  accounts: readonly { id: string; name: string; institution?: { name: string } | null }[]
): { account: string | null; created: number; lastUsed: number } {
  const account = format.accountId
    ? accounts.find((candidate) => candidate.id === format.accountId)
    : undefined;
  const institution = account?.institution?.name.trim();
  return {
    account: account ? [account.name, institution].filter(Boolean).join(' · ') : null,
    created: format.createdAt,
    lastUsed: format.lastUsedAt,
  };
}
