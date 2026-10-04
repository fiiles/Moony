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
};

/**
 * Keys with a second sentence that works the detail into the text ("držíte 2,5")
 * instead of getting it appended in parentheses; used when there is a detail.
 */
const SENTENCE_WITH_DETAIL: Readonly<Record<string, string>> = {
  'importWizard.row.sellExceedsHoldings': 'importWizard.row.sellExceedsHoldingsHeld',
  'importWizard.row.currencyMismatch': 'importWizard.row.currencyMismatchPosition',
};

const DUPLICATE_KEY_PREFIX = 'importWizard.row.duplicate';
const ROW_KEY = /importWizard\.row\.[A-Za-z0-9]+/;

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
  // Every kind of duplicate reads the same; the detail would only be a rule name.
  if (message.key.startsWith(DUPLICATE_KEY_PREFIX)) {
    return t(DUPLICATE_KEY_PREFIX, { defaultValue: message.key }) as string;
  }

  const key = KEY_ALIASES[message.key] ?? message.key;
  const translate = key.startsWith('validation.') ? tc : t;
  const raw = message.detail?.trim() ?? '';
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
 * row message (`importWizard.row.*`), which only the `stocks` namespace knows.
 */
export function importErrorText(error: unknown, context: RowMessageContext): string {
  const raw =
    error instanceof ApiError ? error.raw : error instanceof Error ? error.message : String(error);
  const key = ROW_KEY.exec(raw)?.[0];
  if (key) return rowMessageText({ line: 0, key, detail: null }, context);
  return translateApiError(error instanceof Error ? error : String(error), context.tc);
}
