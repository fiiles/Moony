/**
 * The import sheet's logic (spec §8.6), pure: what a file is, which account it goes to, what the
 * done step counts, and the small decisions the flow's stages hang on. The sheet itself
 * (`ImportSheet`, `useImportFlow`) only wires these to the desktop's parsers and commands.
 */
import type {
  AccountMatch,
  CsvImportResult,
  CsvPreviewResult,
  CsvPreviewRow,
} from '@shared/schema';
import {
  isMappingComplete,
  mappingFromInspection,
  uniqueHeaders,
} from '@/components/bank-accounts/csv-import/import-config';
import { daysBetween } from '@/utils/milestones';

/**
 * The stages of the import sheet. `recognize` ("Takhle tomu rozumím") and `correct` (the list of
 * fields and the per-field correction) belong to the guided recognition of a CSV no preset
 * describes (`recognition.ts`, `use-recognition.ts`).
 */
export type ImportStage =
  | 'pick'
  | 'reading'
  | 'account'
  | 'statement'
  | 'recognize'
  | 'correct'
  | 'preview'
  | 'importing'
  | 'done'
  | 'unreadable';

/** Where an import was asked for: an account's button, the Údržba card, Nastavení, a shared file. */
export type ImportSource = 'account' | 'upkeep' | 'settings' | 'shared';

export interface ImportRequest {
  /** The account the file goes to, when the entry point knows it. */
  accountId?: string;
  /** A file that is already at hand (a shared one); without it the sheet starts on the picker. */
  path?: string;
  source: ImportSource;
  /**
   * The label of the screen the sheet opened on, when it is not the done step's targets' own parent
   * (Přehled, Nastavení › Import): the account or the queue it opens then names it on its back button.
   */
  from?: string;
}

/** What a file's name says it is; the content decides the rest (a statement is told from its content). */
export type FileKind = 'readable' | 'pdf' | 'image';

const IMAGE_EXTENSIONS: ReadonlySet<string> = new Set([
  'heic',
  'heif',
  'jpg',
  'jpeg',
  'png',
  'gif',
  'webp',
]);

/**
 * A PDF and a photo or scan cannot be read, and saying so at once beats a parser error. Anything
 * else — `.STA`, `.940`, no extension at all — is tried as text.
 */
export function fileKind(name: string): FileKind {
  const extension = /\.([^./\\]+)$/.exec(name.trim())?.[1].toLowerCase();
  if (extension === 'pdf') return 'pdf';
  if (extension && IMAGE_EXTENSIONS.has(extension)) return 'image';
  return 'readable';
}

/** What `chooseAccount` needs of an account. */
export interface ImportAccount {
  id: string;
  institutionId: string | null;
  terminationDate: number | null;
}

/** An account is open until its termination date has passed (the day itself still counts). */
export function isOpenAccount(
  account: Pick<ImportAccount, 'terminationDate'>,
  todayDay: number
): boolean {
  return account.terminationDate === null || daysBetween(todayDay, account.terminationDate) >= 0;
}

export type AccountChoiceReason = 'preselected' | 'statement' | 'institution' | 'only' | 'ask';

/**
 * The account a file goes to, without asking when it can be told:
 * 1. the one the entry point named (an account's own button);
 * 2. the open account a statement says it belongs to;
 * 3. the one open account of the bank the CSV preset is for;
 * 4. the only open account there is.
 * A closed account is never chosen by 2–4. `ask` (no account) when none of those decides.
 */
export function chooseAccount(input: {
  preselected?: string;
  statement?: { match: AccountMatch; matchedAccountId: string | null };
  presetInstitutionId?: string | null;
  accounts: readonly ImportAccount[];
  todayDay: number;
}): { accountId: string | null; reason: AccountChoiceReason } {
  if (input.preselected) return { accountId: input.preselected, reason: 'preselected' };

  const open = input.accounts.filter((account) => isOpenAccount(account, input.todayDay));

  const { statement } = input;
  if (
    statement &&
    (statement.match === 'selected' || statement.match === 'other') &&
    statement.matchedAccountId !== null &&
    open.some((account) => account.id === statement.matchedAccountId)
  ) {
    return { accountId: statement.matchedAccountId, reason: 'statement' };
  }

  if (input.presetInstitutionId) {
    const ofBank = open.filter((account) => account.institutionId === input.presetInstitutionId);
    if (ofBank.length === 1) return { accountId: ofBank[0].id, reason: 'institution' };
  }

  if (open.length === 1) return { accountId: open[0].id, reason: 'only' };
  return { accountId: null, reason: 'ask' };
}

export interface ImportFacts {
  imported: number;
  /** Duplicates that were skipped (the phone never imports one anyway). */
  duplicates: number;
  errors: number;
  /** Imported payments the rules left without a category. */
  uncategorized: number;
  /** Imported payments the rules gave a category. */
  categorized: number;
}

/** The three stats and the categorization sentence of the done step. */
export function importFacts(result: CsvImportResult): ImportFacts {
  return {
    imported: result.importedCount,
    duplicates: result.skippedDuplicates.length,
    errors: result.errorCount,
    uncategorized: result.uncategorizedCount,
    categorized: Math.max(0, result.importedCount - result.uncategorizedCount),
  };
}

/**
 * A preview row's amount in the shape `signedAmountText` takes: the sign comes from the value (a
 * preview row has no type) and the currency is the row's own, else the account's. `null` for a
 * row whose amount the file did not give or could not be read.
 */
export function previewAmount(
  row: Pick<CsvPreviewRow, 'amount' | 'currency'>,
  fallbackCurrency: string
): { type: 'credit' | 'debit'; amount: string; currency: string } | null {
  if (row.amount === null || !Number.isFinite(Number(row.amount))) return null;
  return {
    type: Number(row.amount) < 0 ? 'debit' : 'credit',
    amount: row.amount,
    currency: row.currency ?? fallbackCurrency,
  };
}

/**
 * A stated amount as it will be stored: the cents it has (at least two), none for a whole number
 * (the desktop's `formatBalance`). `money` is `useMoneyText()`.
 */
export function exactMoneyText(
  money: (amount: number, currency: string, options?: { decimals?: number }) => string,
  amount: string,
  currency: string
): string {
  const value = Number(amount);
  if (!Number.isFinite(value)) return amount;
  const written = /\.(\d+)\s*$/.exec(amount.trim())?.[1].length ?? 0;
  const decimals = written === 0 ? 0 : Math.min(8, Math.max(2, written));
  return money(value, currency, { decimals });
}

/** A statement file asks something before its preview: which statement, or how its dates read. */
export function statementNeedsStage(inspection: {
  statements: readonly unknown[];
  dateOrderAmbiguous: boolean;
}): boolean {
  return inspection.statements.length > 1 || inspection.dateOrderAmbiguous;
}

/**
 * Whether a CSV goes straight to its preview (spec §8.6, rung 2): a bank preset matched its header
 * signature and names a date and an amount (or a debit and a credit column). Any other CSV is
 * recognized from its content first, even when its headers happen to name both columns.
 */
export function presetReadable(inspection: CsvPreviewResult): boolean {
  return (
    inspection.detectedPresetId !== null &&
    !inspection.dateFormatAmbiguous &&
    isMappingComplete(mappingFromInspection(inspection))
  );
}

/**
 * Whether there is anything to recognize in a CSV: two named columns (a statement has at least a
 * date and an amount) and a row to look at. An empty file, a file of one column and a bare header
 * are no statement, and say so instead of opening a correction with nothing to pick from.
 */
export function csvRecognizable(inspection: CsvPreviewResult): boolean {
  return uniqueHeaders(inspection.headers).length >= 2 && inspection.sampleRows.length > 0;
}

/**
 * Whether a file the Files picker handed back is still wanted: the sheet that asked is open and
 * no read (or close) has bumped the flow's generation since the picker was opened. A file picked
 * after the sheet went away (a lock, Android back) is a plaintext copy nobody will read, so it
 * is discarded at once instead of adopted.
 */
export function pickedFileStillWanted(input: {
  open: boolean;
  /** The flow's read generation when the picker was opened. */
  askedAt: number;
  /** The generation now. */
  now: number;
}): boolean {
  return input.open && input.askedAt === input.now;
}

/**
 * "Vypadá to správně" went to fetch the accounts. What comes back is used only while the sheet is
 * still on the recognition's rows and nothing superseded it: the person may have tapped "Opravit…"
 * meanwhile, and the stage the continuation set then would bury the correction they opened.
 */
export function recognitionAnswerWanted(input: {
  open: boolean;
  /** The stage the sheet is in when the answer arrives. */
  stage: ImportStage;
  /** The flow's read generation when "Vypadá to správně" was tapped. */
  startedAt: number;
  /** The generation now (every question opened or answered bumps it). */
  now: number;
}): boolean {
  return input.open && input.stage === 'recognize' && input.startedAt === input.now;
}

/** Rows of the preview the phone asks the backend for (and shows). */
export const PREVIEW_ROWS = 50;
