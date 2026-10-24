/**
 * The rules of the phone's documents (spec §8.8 "Dokumenty", §9): which picked files are turned
 * away before anything is read, what a new document is called, and the sub-line of a row. Pure, so
 * the camera, the picker and the section share one answer.
 */
import type { Formatters } from '@/lib/format';

/** The attachment store's limit (`validation.attachmentTooLarge`): the phone refuses before reading. */
export const MAX_DOCUMENT_BYTES = 100 * 1024 * 1024;

/** Where a file came from: the camera names every capture "image.jpg", a picked file has its own name. */
export type DocumentSource = 'camera' | 'picker';

/** Why a file never opens the add sheet: HEIC is not supported (owner ruling), no bytes, too many bytes. */
export type PickedFileProblem = 'heic' | 'empty' | 'tooLarge';

/** What the checks need of a `File`. */
interface PickedFile {
  name: string;
  type: string;
  size: number;
}

// As `src/utils/photo-files.ts` (the gallery's rule): the type, or the name when the picker gives none.
const HEIC_TYPE = /^image\/hei[cf]$/;
const HEIC_EXTENSION = /\.hei[cf]$/i;

/**
 * The reason a picked or captured file is not offered for saving, or `null`. Everything else is
 * accepted: a document can be of any type, and one the viewer cannot show is still kept (the
 * viewer says so). A HEIC photo is named before an empty one, since the format is what the person
 * can do something about.
 */
export function pickedFileProblem(file: PickedFile): PickedFileProblem | null {
  if (HEIC_TYPE.test(file.type) || HEIC_EXTENSION.test(file.name)) return 'heic';
  if (file.size === 0) return 'empty';
  if (file.size > MAX_DOCUMENT_BYTES) return 'tooLarge';
  return null;
}

/** The backend's limit on a document's name: `name.len() > 200` in Rust counts UTF-8 bytes. */
const MAX_NAME_BYTES = 200;

/** `name` trimmed, then cut to 200 UTF-8 bytes (never more than 200 characters), never inside a character. */
function clampName(name: string): string {
  const encoder = new TextEncoder();
  let kept = '';
  let bytes = 0;
  for (const character of name.trim()) {
    bytes += encoder.encode(character).length;
    if (bytes > MAX_NAME_BYTES) break;
    kept += character;
  }
  return kept.trim();
}

/** The last extension of a file name: a dot and at least one character that is no dot or space, to the end. */
const EXTENSION = /\.[^./\\\s]+$/;

/**
 * The name a new document starts with: a picked file's own name without its extension
 * ("Smlouva 2024.pdf" → "Smlouva 2024"); `photoName` — the caller's dated "Foto 24. 10. 2026" — for
 * a camera capture (every one is called "image.jpg") and for a picked name with nothing left once
 * the extension is gone (".pdf"). At most 200 characters, as the backend allows.
 */
export function documentNameFrom(
  fileName: string,
  source: DocumentSource,
  photoName: string
): string {
  if (source === 'camera') return clampName(photoName);
  const base = clampName(fileName.replace(EXTENSION, ''));
  return base === '' ? clampName(photoName) : base;
}

const KILOBYTE = 1024;
const MEGABYTE = 1024 * 1024;

/** "512 B", "830 kB", "1,2 MB": the desktop's units, the number in the language's own form. */
export function fileSizeText(bytes: number, fmt: Pick<Formatters, 'number'>): string {
  if (bytes < KILOBYTE) return `${fmt.number(bytes)} B`;
  const kilobytes = Math.round(bytes / KILOBYTE);
  // 1 048 575 bytes round to 1024 kB: that is a megabyte
  if (kilobytes < KILOBYTE) return `${fmt.number(kilobytes)} kB`;
  return `${fmt.number(bytes / MEGABYTE, { maximumFractionDigits: 1 })} MB`;
}

/** What a document row's sub-line reads. */
interface DocumentSubInput {
  fileType: string;
  fileSize: number | null;
  /** A real instant (the backend stamps it with the clock), not a calendar day. */
  uploadedAt: number;
}

/**
 * The sub-line of a document row: the type's label · the size · the upload day ("smlouva · 1,2 MB ·
 * 24. 10. 2026"). The size is left out when it is not known. The day is the local one — the upload
 * is a real instant, so a document added at 00:30 is not dated the day before.
 */
export function documentSub(
  doc: DocumentSubInput,
  typeLabel: (fileType: string) => string,
  fmt: Pick<Formatters, 'number' | 'date'>
): string {
  return [
    typeLabel(doc.fileType),
    doc.fileSize ? fileSizeText(doc.fileSize, fmt) : '',
    fmt.date(new Date(doc.uploadedAt * 1000)),
  ]
    .filter(Boolean)
    .join(' · ');
}

/**
 * The picker's `accept` for a document: the desktop's document filter (pdf, doc, docx, jpg, jpeg,
 * png, gif). It names JPEG and not HEIC, so WebKit converts a Photo Library original to JPEG
 * instead of handing over the HEIC (one that arrives anyway is still refused by `pickedFileProblem`).
 */
export const DOCUMENT_ACCEPT = 'application/pdf,.pdf,.doc,.docx,image/jpeg,image/png,image/gif';

/** One tap of the viewer: when (a pointer event's `timeStamp`, ms) and where (client px). */
export interface TapPoint {
  time: number;
  x: number;
  y: number;
}

/** A second tap counts as a double tap within this long of the first … */
const DOUBLE_TAP_MS = 300;
/** … and this far from it. */
const DOUBLE_TAP_PX = 30;

/** Whether `tap` is the second tap of a double tap that began with `previous`. */
export function isDoubleTap(previous: TapPoint | null, tap: TapPoint): boolean {
  if (previous === null) return false;
  const elapsed = tap.time - previous.time;
  return (
    elapsed >= 0 &&
    elapsed <= DOUBLE_TAP_MS &&
    Math.hypot(tap.x - previous.x, tap.y - previous.y) <= DOUBLE_TAP_PX
  );
}

/**
 * Where a scroll box should be scrolled (one axis) after its content went from `from`× to `to`×
 * its width, so that the point `pointer` px into the box stays under the finger. Never below 0.
 */
export function scrollAfterZoom(scroll: number, pointer: number, from: number, to: number): number {
  return Math.max(0, ((scroll + pointer) / from) * to - pointer);
}
