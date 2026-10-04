import type { TFunction } from 'i18next';
import type { StockRowMessage } from '@shared/schema';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { formatDecimalText } from './import-config';

/**
 * Translating the row messages of the stock import.
 *
 * The backend never sends display text: `importWizard.row.*` keys live in the
 * `stocks` namespace, `validation.*` keys in `common`. An unknown key comes
 * back unchanged so a missing translation stays visible instead of rendering
 * an empty string.
 */

/** Row-specific wording for a generic validation key (the form sentence reads wrongly in a row). */
const KEY_ALIASES: Readonly<Record<string, string>> = {
  'validation.currencyInvalid': 'importWizard.row.currencyInvalid',
  'validation.tickerInvalid': 'importWizard.row.tickerInvalid',
};

/** Keys whose detail is a technical reader error: the line says enough. */
const NO_DETAIL_KEYS: ReadonlySet<string> = new Set(['importWizard.row.cannotParse']);

/**
 * Keys with a second sentence that works the detail into the text ("držíte 2,5")
 * instead of getting it appended in parentheses; used when there is a detail.
 */
const SENTENCE_WITH_DETAIL: Readonly<Record<string, string>> = {
  'importWizard.row.sellExceedsHoldings': 'importWizard.row.sellExceedsHoldingsHeld',
  'importWizard.row.currencyMismatch': 'importWizard.row.currencyMismatchPosition',
  // the detail is an ISIN cell that is none: "no symbol, and the ISIN … is not valid"
  'importWizard.row.symbolMissing': 'importWizard.row.symbolMissingIsin',
};

/** The duplicate messages the backend sends; any other `duplicate…` key reads as the first. */
const DUPLICATE_KEYS: readonly string[] = [
  'importWizard.row.duplicate',
  'importWizard.row.duplicateById',
];
const DUPLICATE_KEY_PREFIX = 'importWizard.row.duplicate';
const ROW_KEY = /importWizard\.row\.[A-Za-z0-9]+/;
/** The abort of a write that found a rule broken: "line 12: <reason>". */
const WRITE_FAILURE = /\bline (\d+): ([\s\S]+)$/;
const TRANSLATABLE_REASON = /^(validation|importWizard\.row)\.[A-Za-z0-9]+$/;

export interface RowMessageContext {
  /** `stocks` namespace. */
  t: TFunction;
  /** `common` namespace. */
  tc: TFunction;
  /** UI locale, for numbers in a detail (`cs-CZ`). */
  locale: string;
}

/**
 * The sentence of one row message, its detail (the unreadable cell, the type
 * value, the held quantity) in parentheses or inside the sentence.
 */
export function rowMessageText(
  message: StockRowMessage,
  { t, tc, locale }: RowMessageContext
): string {
  // The detail of a duplicate is a broker id or nothing: the sentence says enough.
  if (message.key.startsWith(DUPLICATE_KEY_PREFIX)) {
    const key = DUPLICATE_KEYS.includes(message.key) ? message.key : DUPLICATE_KEYS[0];
    return t(key, { defaultValue: message.key }) as string;
  }

  const key = KEY_ALIASES[message.key] ?? message.key;
  const translate = key.startsWith('validation.') ? tc : t;
  const raw = NO_DETAIL_KEYS.has(key) ? '' : (message.detail?.trim() ?? '');
  const detail =
    key === 'importWizard.row.sellExceedsHoldings' && raw !== ''
      ? formatDecimalText(raw, locale)
      : raw;

  const detailed = SENTENCE_WITH_DETAIL[key];
  if (detailed && detail !== '') {
    return t(detailed, { defaultValue: detailed, detail }) as string;
  }
  const text = translate(key, { defaultValue: key, detail }) as string;
  return detail !== '' ? `${text} (${detail})` : text;
}

/** "Řádek 8: Neplatné datum (31-02-2026)": the line is the 1-based file line, header included. */
export function rowMessageLine(message: StockRowMessage, context: RowMessageContext): string {
  return context.t('importWizard.result.line', {
    line: message.line,
    message: rowMessageText(message, context),
  }) as string;
}

/**
 * What to tell the user about a failed command. `ApiError` messages are already
 * translated, except a rule the write found broken: the backend then names the
 * row ("line 12: …") and gives a key as the reason, which `ApiError` cannot
 * translate (`importWizard.row.*` lives in the `stocks` namespace).
 */
export function importErrorText(error: unknown, context: RowMessageContext): string {
  const raw =
    error instanceof ApiError ? error.raw : error instanceof Error ? error.message : String(error);

  // "line 12: validation.sellExceedsHoldings": say which row, in words. A reason that is
  // not a key (curated English, technical text) is not shown.
  const failure = WRITE_FAILURE.exec(raw);
  if (failure) {
    const reason = failure[2].trim();
    const key = TRANSLATABLE_REASON.test(reason) ? reason : 'importWizard.row.writeFailed';
    return rowMessageLine({ line: Number(failure[1]), key, detail: null }, context);
  }

  const key = ROW_KEY.exec(raw)?.[0];
  if (key) return rowMessageText({ line: 0, key, detail: null }, context);
  return translateApiError(error instanceof Error ? error : String(error), context.tc);
}
