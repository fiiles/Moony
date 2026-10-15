/**
 * How the app shows an attachment (D3 of the encrypted-attachments spec): images and PDFs open
 * in the in-app viewer; every other type opens in its system app through a temporary decrypted
 * copy, after a one-time warning.
 */

export type ViewerKind = 'image' | 'pdf' | 'external';

/** Image types every platform's webview renders. HEIC and SVG are deliberately not among them. */
const VIEWABLE_IMAGES = new Set([
  'image/jpeg',
  'image/png',
  'image/gif',
  'image/webp',
  'image/bmp',
]);

export function viewerKind(mimeType: string): ViewerKind {
  if (mimeType === 'application/pdf') return 'pdf';
  if (VIEWABLE_IMAGES.has(mimeType)) return 'image';
  return 'external';
}

/** The default file name of a saved copy: the display name plus the original extension. */
export function defaultFileName(name: string, extension: string | null): string {
  const base = name.trim().replace(/[\\/:*?"<>|]/g, '_') || 'document';
  if (!extension) return base;
  return base.toLowerCase().endsWith(`.${extension}`) ? base : `${base}.${extension}`;
}

/** Device setting: the user has seen the "temporary unencrypted copy" warning and opted out. */
export const OPEN_ELSEWHERE_CONFIRMED_KEY = 'moony-open-elsewhere-confirmed';
