/**
 * Why a PDF preview did not draw. A document protected by an opening password (insurers and banks
 * often send their PDFs so, locked with the client's birth number or a password from a text
 * message) is not broken: pdf.js refuses it with a `PasswordException` until it is given the
 * password, so the viewer asks for it instead of saying the preview failed.
 */
export type PdfFailure = 'needsPassword' | 'wrongPassword' | 'failed';

/** pdf.js's `PasswordResponses`: no password was given, or the one given did not open it. */
const NEED_PASSWORD = 1;
const INCORRECT_PASSWORD = 2;

/** What a pdf.js rejection means for the viewer. */
export function pdfFailure(error: unknown): PdfFailure {
  if (!(error instanceof Error) || error.name !== 'PasswordException') return 'failed';
  const code = (error as Error & { code?: unknown }).code;
  if (code === NEED_PASSWORD) return 'needsPassword';
  if (code === INCORRECT_PASSWORD) return 'wrongPassword';
  return 'failed';
}
