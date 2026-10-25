import { describe, it, expect } from 'vitest';
import { ApiError, isAuthApiError, normalizeApiError, parseKeyed, toApiError } from './api-error';

// Stand-in for the i18next `t` bound to the `common` namespace. Like the real one called with
// `{ defaultValue: '' }`, it returns '' for unknown keys.
const dictionary: Record<string, string> = {
  'validation.amountRequired': 'Amount is required',
  'auth.wrongPassword': 'The password you entered is incorrect. Please try again.',
  'errors.notFound': 'The item no longer exists. Refresh and try again.',
  'errors.database': 'The local database reported an error. Check the logs for details.',
  'errors.externalApi': 'A market-data service did not respond. Try again later.',
  'errors.internal': 'Something went wrong inside Moony. Check the logs for details.',
  'errors.encryption': 'Moony could not encrypt or decrypt your data.',
  'errors.validation': 'The entered data is not valid.',
  'errors.auth': 'Authentication failed. Try unlocking Moony again.',
  'errors.unknown': 'An unexpected error occurred. Check the logs for details.',
};
const t = (key: string) => dictionary[key] ?? '';

describe('normalizeApiError', () => {
  it('translates a validation key wrapped in the backend prefix', () => {
    const result = normalizeApiError('Validation error: validation.amountRequired', t);
    expect(result).toEqual({
      kind: 'validation',
      key: 'validation.amountRequired',
      message: 'Amount is required',
      raw: 'Validation error: validation.amountRequired',
    });
  });

  it('translates a stable auth key', () => {
    const result = normalizeApiError('Authentication error: auth.wrongPassword', t);
    expect(result.kind).toBe('auth');
    expect(result.key).toBe('auth.wrongPassword');
    expect(result.message).toBe('The password you entered is incorrect. Please try again.');
  });

  it('translates a bare key without the prefix', () => {
    const result = normalizeApiError('validation.amountRequired', t);
    expect(result.kind).toBe('validation');
    expect(result.message).toBe('Amount is required');
  });

  it('falls back to the generic validation message when the key has no translation', () => {
    const result = normalizeApiError('Validation error: validation.somethingNew', t);
    expect(result.kind).toBe('validation');
    expect(result.key).toBe('validation.somethingNew');
    expect(result.message).toBe('The entered data is not valid.');
  });

  it('falls back to the generic auth message when the auth key has no translation', () => {
    const result = normalizeApiError('Authentication error: auth.somethingNew', t);
    expect(result.kind).toBe('auth');
    expect(result.message).toBe('Authentication failed. Try unlocking Moony again.');
  });

  it('keeps a raw English validation remainder as the message', () => {
    const result = normalizeApiError("Validation error: Invalid interest rate '150'", t);
    expect(result.kind).toBe('validation');
    expect(result.key).toBeUndefined();
    expect(result.message).toBe("Invalid interest rate '150'");
  });

  it('uses the generic validation message when the remainder is empty', () => {
    expect(normalizeApiError('Validation error: ', t).message).toBe(
      'The entered data is not valid.'
    );
  });

  it.each([
    ['Not found: Bank account not found', 'notFound', 'errors.notFound'],
    ['Database error: no such table: foo', 'database', 'errors.database'],
    ['Authentication error: Database is locked', 'auth', 'errors.auth'],
    [
      'External API error: Yahoo parse error: x (body: <html>)',
      'externalApi',
      'errors.externalApi',
    ],
    ['Encryption error: bad nonce', 'encryption', 'errors.encryption'],
    ['Internal error: Failed to get app data dir: nope', 'internal', 'errors.internal'],
  ] as const)('maps "%s" to kind %s with the generic message', (raw, kind, genericKey) => {
    const result = normalizeApiError(raw, t);
    expect(result.kind).toBe(kind);
    expect(result.message).toBe(dictionary[genericKey]);
    expect(result.raw).toBe(raw);
  });

  it('never lets raw rusqlite text reach the message', () => {
    const raw =
      'Database error: UNIQUE constraint failed: bank_accounts.iban (code 2067) near "INSERT"';
    const result = normalizeApiError(raw, t);
    expect(result.kind).toBe('database');
    expect(result.message).not.toContain('UNIQUE');
    expect(result.message).not.toContain('bank_accounts');
    expect(result.raw).toBe(raw);
  });

  it('never lets technical internal details reach the message', () => {
    const result = normalizeApiError(
      'Internal error: IO error: /Users/me/Library/db (os error 2)',
      t
    );
    expect(result.message).not.toContain('/Users');
    expect(result.message).not.toContain('os error');
  });

  it('does not treat a translated key inside another kind as a leak', () => {
    const result = normalizeApiError('Not found: validation.amountRequired', t);
    expect(result.kind).toBe('notFound');
    expect(result.message).toBe('Amount is required');
  });

  it('classifies unprefixed text as unknown with the generic message', () => {
    const raw = 'Failed to persist learned payee: disk I/O error';
    const result = normalizeApiError(raw, t);
    expect(result.kind).toBe('unknown');
    expect(result.message).toBe('An unexpected error occurred. Check the logs for details.');
    expect(result.raw).toBe(raw);
  });

  it('does not mistake file names that merely start with "auth." for keys', () => {
    const result = normalizeApiError('Internal error: Failed to read auth.json', t);
    expect(result.kind).toBe('internal');
    expect(result.key).toBeUndefined();
    expect(result.message).toBe(dictionary['errors.internal']);
  });

  it('falls back to a built-in English sentence when even the generic key is missing', () => {
    const result = normalizeApiError('Database error: boom', () => '');
    expect(result.kind).toBe('database');
    expect(result.message).not.toBe('');
    expect(result.message).not.toContain('boom');
  });
});

describe('ApiError / toApiError', () => {
  it('builds an ApiError carrying kind, key, raw and the localized message', () => {
    const error = toApiError('Validation error: validation.amountRequired', t);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toBeInstanceOf(Error);
    expect(error.message).toBe('Amount is required');
    expect(error.kind).toBe('validation');
    expect(error.key).toBe('validation.amountRequired');
    expect(error.raw).toBe('Validation error: validation.amountRequired');
  });

  it('stringifies to the localized message, not "ApiError: ..."', () => {
    const error = toApiError('Not found: Loan not found', t);
    expect(String(error)).toBe('The item no longer exists. Refresh and try again.');
  });

  it('accepts non-string thrown values', () => {
    const error = toApiError(new Error('Internal error: oops'), t);
    expect(error.kind).toBe('internal');
    expect(error.raw).toBe('Internal error: oops');
    expect(toApiError({ weird: true }, t).kind).toBe('unknown');
  });

  it('passes through an existing ApiError unchanged', () => {
    const first = toApiError('Database error: x', t);
    expect(toApiError(first, t)).toBe(first);
  });
});

describe('browser dev bridge errors', () => {
  it('are shown verbatim because they instruct the developer, not the user', () => {
    const raw = '[browser-bridge] Tauri window is not responding (minimized or suspended?)';
    const n = normalizeApiError(raw, () => '');
    expect(n.kind).toBe('unknown');
    expect(n.message).toBe(raw);
  });
});

describe('isAuthApiError', () => {
  it('is true for credential failures (keyed or unkeyed Authentication errors)', () => {
    expect(isAuthApiError(toApiError('Authentication error: auth.wrongPassword', t))).toBe(true);
    expect(isAuthApiError(toApiError('Authentication error: something odd', t))).toBe(true);
  });

  it('is false for every other kind and for non-ApiErrors', () => {
    expect(isAuthApiError(toApiError('Database error: locked', t))).toBe(false);
    expect(isAuthApiError(toApiError('Encryption error: bad block', t))).toBe(false);
    expect(isAuthApiError(toApiError('Validation error: validation.amountRequired', t))).toBe(
      false
    );
    expect(isAuthApiError(new Error('Authentication error: auth.wrongPassword'))).toBe(false);
    expect(isAuthApiError(null)).toBe(false);
  });
});

describe('keyed validation errors with values', () => {
  const translations: Record<string, string> = {
    'validation.currencyMismatch': 'held in {{expected}}, transaction in {{actual}}',
    'validation.stockImportLine': 'Line {{line}}: {{reason}}',
    'validation.stockImportRowFailed': 'the row could not be saved.',
    'errors.validation': 'The entered data is not valid.',
  };
  // Like i18next: interpolates {{name}} from the options, '' for an unknown key.
  const tv = (key: string, options?: Record<string, unknown>) => {
    const template = translations[key];
    if (!template) return '';
    return template.replace(/\{\{(\w+)\}\}/g, (_match, name: string) =>
      String(options?.[name] ?? '')
    );
  };

  it('splits a key from the JSON values after the bar', () => {
    expect(parseKeyed('validation.currencyMismatch|{"expected":"USD","actual":"EUR"}')).toEqual({
      key: 'validation.currencyMismatch',
      params: { expected: 'USD', actual: 'EUR' },
    });
    expect(parseKeyed('validation.required')).toEqual({ key: 'validation.required', params: {} });
    expect(parseKeyed('validation.x|{oops}')).toEqual({ key: 'validation.x', params: {} });
    expect(parseKeyed('Invalid interest rate')).toBeNull();
    expect(parseKeyed('validation.x|[1]')).toBeNull();
  });

  it('passes the values to the translator', () => {
    const raw = 'Validation error: validation.currencyMismatch|{"actual":"EUR","expected":"USD"}';
    const result = normalizeApiError(raw, tv);
    expect(result.key).toBe('validation.currencyMismatch');
    expect(result.message).toBe('held in USD, transaction in EUR');
  });

  it('translates a key carried inside a value', () => {
    const raw =
      'Validation error: validation.stockImportLine|{"line":3,"reason":"validation.stockImportRowFailed"}';
    expect(normalizeApiError(raw, tv).message).toBe('Line 3: the row could not be saved.');
  });

  it('translates a keyed value that carries values of its own', () => {
    const raw =
      'Validation error: validation.stockImportLine|{"line":7,"reason":"validation.currencyMismatch|{\\"actual\\":\\"EUR\\",\\"expected\\":\\"USD\\"}"}';
    expect(normalizeApiError(raw, tv).message).toBe('Line 7: held in USD, transaction in EUR');
  });

  it('falls back to the generic message for a key without a translation', () => {
    const result = normalizeApiError('Validation error: validation.newKey|{"a":1}', tv);
    expect(result.key).toBe('validation.newKey');
    expect(result.message).toBe('The entered data is not valid.');
  });
});
