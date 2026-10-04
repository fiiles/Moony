import { describe, it, expect } from 'vitest';
import type { TFunction } from 'i18next';
import { translateApiError } from './translate-api-error';
import { toApiError } from './api-error';

// Minimal stand-in for the `common` namespace translator: returns '' for unknown keys,
// which is what `t(key, { defaultValue: '' })` does in the real thing.
const dictionary: Record<string, string> = {
  'validation.passwordMinLength': 'Password must be at least 8 characters',
  'auth.wrongPassword': 'Incorrect password.',
  'auth.invalidRecoveryKey': 'Invalid recovery key.',
  'auth.keyFilesCorrupted': 'Key files are damaged.',
};
const t = ((key: string) => dictionary[key] ?? '') as unknown as TFunction;

describe('translateApiError', () => {
  it('translates validation keys wrapped in the backend prefix', () => {
    expect(translateApiError('Validation error: validation.passwordMinLength', t)).toBe(
      'Password must be at least 8 characters'
    );
  });

  it.each([
    ['Authentication error: auth.wrongPassword', 'Incorrect password.'],
    ['Authentication error: auth.invalidRecoveryKey', 'Invalid recovery key.'],
    ['Authentication error: auth.keyFilesCorrupted', 'Key files are damaged.'],
  ])('translates the stable auth key in %s', (message, expected) => {
    expect(translateApiError(message, t)).toBe(expected);
    expect(translateApiError(new Error(message), t)).toBe(expected);
  });

  it('translates a bare auth key', () => {
    expect(translateApiError('auth.wrongPassword', t)).toBe('Incorrect password.');
  });

  it('falls back to the original message when the key has no translation', () => {
    const message = 'Authentication error: auth.somethingNew';
    expect(translateApiError(message, t)).toBe(message);
  });

  it('falls back to the original message for plain errors', () => {
    expect(translateApiError('Database error: disk is full', t)).toBe(
      'Database error: disk is full'
    );
  });

  it('does not treat file names that merely start with "auth." as keys', () => {
    const message = 'Failed to read /data/auth.json';
    expect(translateApiError(message, t)).toBe(message);
  });

  it('returns the already-localized message of an ApiError untouched', () => {
    const apiError = toApiError('Database error: UNIQUE constraint failed: x.y', () => '');
    expect(translateApiError(apiError, t)).toBe(apiError.message);
    expect(translateApiError(apiError, t)).not.toContain('UNIQUE');
  });
});
