import type { TFunction } from 'i18next';
import { ApiError } from '@/lib/api-error';

/**
 * Translates API error messages that contain i18n keys.
 *
 * The backend returns errors using translation keys like:
 * - "validation.principalPositive"
 * - "validation.loanNameRequired"
 * - "auth.wrongPassword", "auth.invalidRecoveryKey", "auth.keyFilesCorrupted"
 *
 * These may be returned as just the key, or wrapped in a message like:
 * - "Validation error: validation.principalPositive"
 * - "Authentication error: auth.wrongPassword"
 *
 * This function detects these patterns and translates them using
 * the common namespace's `validation` and `auth` sections.
 *
 * Errors thrown by `tauriInvoke` are `ApiError`s whose message is already localized
 * (`src/lib/api-error.ts`), so they are returned as-is. Plain errors and strings (e.g. from
 * non-Tauri code paths) still go through the key matching below.
 *
 * @param error - The error object or error message string
 * @param t - The i18next translation function (should be from common namespace or support namespaced keys)
 * @returns The translated error message, or original message if no translation found
 */
export function translateApiError(error: Error | string, t: TFunction): string {
  if (error instanceof ApiError) return error.message;

  const message = typeof error === 'string' ? error : error.message;

  // Check if the message contains a translation key pattern (e.g., validation.xxx or auth.xxx)
  // This handles both exact matches and messages like "Validation error: validation.xxx".
  // Look-alikes (e.g. a file name "auth.json") have no translation and fall through below.
  const keyMatch = message.match(/\b((?:validation|auth)\.[a-zA-Z]+)\b/);
  if (keyMatch) {
    const key = keyMatch[1];
    const translated = t(key, { defaultValue: '' });
    // If translation was found (not empty), return it
    if (translated && translated !== key) {
      return translated;
    }
  }

  // Return original message if no translation key found
  return message;
}
