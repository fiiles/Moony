import type { TFunction } from 'i18next';
import type { CsvRowMessage } from '@shared/schema';
import type { ImportKind } from '@/components/bank-accounts/csv-import/import-config';

/**
 * Translates the `key` of a CSV row message.
 *
 * The backend never sends display text: `csvImport.*` keys live in the
 * `bank_accounts` namespace (`t`), `validation.*` keys in `common` (`tc`). An
 * unknown key comes back unchanged so a missing translation stays visible
 * instead of rendering an empty string.
 */
export function translateRowMessage(message: CsvRowMessage, t: TFunction, tc: TFunction): string {
  const translate = message.key.startsWith('validation.') ? tc : t;
  return translate(message.key, { defaultValue: message.key });
}

/**
 * "Row 8: The date could not be read (31-02-2026)" — the one place every list
 * of import problems goes through. For a CSV `line` is the 1-based file
 * line, header included, so it matches what the user sees in a text editor; for
 * a statement it is the payment's ordinal ("Payment 3: ...").
 * The raw `detail` (the unreadable cell, the filtered state, ...) follows in
 * parentheses when the backend sent one.
 */
export function formatRowMessage(
  message: CsvRowMessage,
  t: TFunction,
  tc: TFunction,
  kind: ImportKind = 'csv'
): string {
  const text = translateRowMessage(message, t, tc);
  const detail = message.detail?.trim();
  // A statement's `line` is the payment's ordinal in the statement (D5), not a file line.
  const prefix = kind === 'statement' ? 'statementImport' : 'csvImport';
  return detail
    ? t(`${prefix}.rowMessageDetail`, { line: message.line, message: text, detail })
    : t(`${prefix}.rowMessage`, { line: message.line, message: text });
}
