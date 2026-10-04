/**
 * Central mapping of backend errors to user-facing messages.
 *
 * The Rust `AppError` (`src-tauri/src/error.rs`) is serialized as its display string:
 * `"<Variant prefix>: <detail>"`. The detail is one of
 *   - an i18n key (`validation.*`, `auth.*`) -> translated through `common.json`,
 *   - curated English text (88 `Validation` messages, a few `Auth` ones),
 *   - technical text (rusqlite, IO, HTTP bodies, file paths) that must never reach the UI.
 *
 * `normalizeApiError` is pure so it can be unit-tested; `tauriInvoke` throws an `ApiError` built
 * from it, which means every toast that shows `error.message` is already localized and safe.
 */

export type ApiErrorKind =
  | 'validation'
  | 'auth'
  | 'notFound'
  | 'database'
  | 'externalApi'
  | 'encryption'
  | 'internal'
  | 'unknown';

export interface NormalizedApiError {
  kind: ApiErrorKind;
  /** The i18n key the backend sent, when the detail was one (translated or not). */
  key?: string;
  /** Localized, user-presentable text. */
  message: string;
  /** Exact text received from the backend, for logs. */
  raw: string;
}

/** Minimal translator: `i18n.t` bound to the `common` namespace. Returns '' for unknown keys. */
export type ApiErrorTranslate = (key: string, options?: Record<string, unknown>) => string;

const PREFIXES: ReadonlyArray<readonly [string, ApiErrorKind]> = [
  ['Database error:', 'database'],
  ['Authentication error:', 'auth'],
  ['Not found:', 'notFound'],
  ['Validation error:', 'validation'],
  ['External API error:', 'externalApi'],
  ['Encryption error:', 'encryption'],
  ['Internal error:', 'internal'],
];

/** Only these key families are produced by the backend and live in `common.json`. */
const I18N_KEY = /^(validation|auth)\.[A-Za-z0-9_]+$/;

/** Used when `errors.<kind>` is missing from the locale files (should never happen). */
const BUILT_IN_FALLBACK: Record<ApiErrorKind, string> = {
  validation: 'The entered data is not valid.',
  auth: 'Authentication failed. Try unlocking Moony again.',
  notFound: 'The item no longer exists. Refresh and try again.',
  database: 'The local database reported an error. Check the logs for details.',
  externalApi: 'A market-data service did not respond. Try again later.',
  encryption: 'Moony could not encrypt or decrypt your data.',
  internal: 'Something went wrong inside Moony. Check the logs for details.',
  unknown: 'An unexpected error occurred. Check the logs for details.',
};

function translateOrEmpty(t: ApiErrorTranslate, key: string): string {
  const translated = t(key, { defaultValue: '' });
  return translated && translated !== key ? translated : '';
}

function genericMessage(kind: ApiErrorKind, t: ApiErrorTranslate): string {
  return translateOrEmpty(t, `errors.${kind}`) || BUILT_IN_FALLBACK[kind];
}

export function normalizeApiError(raw: string, t: ApiErrorTranslate): NormalizedApiError {
  const text = raw.trim();

  // Dev-only browser bridge errors ("Tauri window is not responding …") are instructions for
  // the developer, not backend errors: show them verbatim instead of the generic message.
  if (text.startsWith('[browser-bridge]')) {
    return { kind: 'unknown', message: text, raw };
  }

  let kind: ApiErrorKind = 'unknown';
  let detail = text;
  for (const [prefix, prefixKind] of PREFIXES) {
    if (text.startsWith(prefix)) {
      kind = prefixKind;
      detail = text.slice(prefix.length).trim();
      break;
    }
  }

  if (I18N_KEY.test(detail)) {
    // Bare keys (no prefix) still tell us the kind.
    if (kind === 'unknown') kind = detail.startsWith('auth.') ? 'auth' : 'validation';
    const translated = translateOrEmpty(t, detail);
    return {
      kind,
      key: detail,
      message: translated || genericMessage(kind, t),
      raw,
    };
  }

  // Curated English text from the backend is safe and more useful than a generic sentence.
  if (kind === 'validation' && detail) {
    return { kind, message: detail, raw };
  }

  return { kind, message: genericMessage(kind, t), raw };
}

/** Error thrown by every Tauri command wrapper. `message` is localized and safe to display. */
export class ApiError extends Error {
  readonly kind: ApiErrorKind;
  readonly key?: string;
  readonly raw: string;

  constructor(normalized: NormalizedApiError, options?: { cause?: unknown }) {
    super(normalized.message, options);
    this.name = 'ApiError';
    this.kind = normalized.kind;
    this.key = normalized.key;
    this.raw = normalized.raw;
  }

  /** `String(error)` is used in several toasts; show the message without an "ApiError:" prefix. */
  override toString(): string {
    return this.message;
  }
}

/**
 * True for credential failures (`Authentication error: …`, e.g. `auth.wrongPassword`). The lock
 * screen shows these inline under the field instead of as a toast.
 */
export function isAuthApiError(error: unknown): error is ApiError {
  return error instanceof ApiError && error.kind === 'auth';
}

function rawText(thrown: unknown): string {
  if (typeof thrown === 'string') return thrown;
  if (thrown instanceof Error) return thrown.message;
  return String(thrown);
}

/** Convert whatever `invoke()` rejected with into an `ApiError`. */
export function toApiError(thrown: unknown, t: ApiErrorTranslate): ApiError {
  if (thrown instanceof ApiError) return thrown;
  return new ApiError(normalizeApiError(rawText(thrown), t), { cause: thrown });
}
