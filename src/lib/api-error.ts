/**
 * Central mapping of backend errors to user-facing messages.
 *
 * The Rust `AppError` (`src-tauri/src/error.rs`) is serialized as its display string:
 * `"<Variant prefix>: <detail>"`. The detail is one of
 *   - an i18n key (`validation.*`, `auth.*`), optionally followed by `|` and a JSON object of
 *     interpolation values -> translated through `common.json`,
 *   - curated English text (a few legacy `Validation` and `Auth` messages; MCP-only sites),
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

/**
 * Only these key families are produced by the backend and live in `common.json`. A key may carry
 * interpolation values as a JSON object after a bar: `validation.currencyMismatch|{"expected":"USD"}`.
 */
const KEYED = /^((?:validation|auth)\.[A-Za-z0-9_]+)(?:\|(\{[\s\S]*\}))?$/;

export interface KeyedMessage {
  key: string;
  params: Record<string, unknown>;
}

/** Splits `key` or `key|{json}`; null for anything else. Unreadable values are dropped, the key stays. */
export function parseKeyed(text: string): KeyedMessage | null {
  const match = KEYED.exec(text.trim());
  if (!match) return null;
  let params: Record<string, unknown> = {};
  if (match[2]) {
    try {
      const parsed: unknown = JSON.parse(match[2]);
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        params = parsed as Record<string, unknown>;
      }
    } catch {
      // the key without its values is still better than the English fallback
    }
  }
  return { key: match[1], params };
}

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

/** The translated text of a keyed message; a value that is itself a key (stock import `reason`) is translated first. */
function translateKeyed(keyed: KeyedMessage, t: ApiErrorTranslate): string {
  const params: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(keyed.params)) {
    const nested = typeof value === 'string' ? parseKeyed(value) : null;
    params[name] = nested ? translateKeyed(nested, t) || value : value;
  }
  const translated = t(keyed.key, { ...params, defaultValue: '' });
  return translated && translated !== keyed.key ? translated : '';
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

  const keyed = parseKeyed(detail);
  if (keyed) {
    // Bare keys (no prefix) still tell us the kind.
    if (kind === 'unknown') kind = keyed.key.startsWith('auth.') ? 'auth' : 'validation';
    return {
      kind,
      key: keyed.key,
      message: translateKeyed(keyed, t) || genericMessage(kind, t),
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
