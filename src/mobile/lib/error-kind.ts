import { ApiError } from '@/lib/api-error';

/**
 * What a phone log line may say about an error: its kind, never its text. A backend error's raw
 * text and a read error's message can carry a path or a file name — a statement shared from the
 * bank, a photographed contract — and those stay out of the console as they stay out of the logs
 * on disk (the plaintext-leaks rule). A backend error gives its kind and its i18n key
 * (`ApiError(validation: validation.attachmentTooLarge)`), any other error its name
 * (`NotReadableError`), anything else its type.
 */
export function errorKind(error: unknown): string {
  if (error instanceof ApiError) {
    return error.key ? `ApiError(${error.kind}: ${error.key})` : `ApiError(${error.kind})`;
  }
  if (error instanceof Error) return error.name;
  return error === null ? 'null' : typeof error;
}
