/**
 * The rules of the phone's documents (spec §8.8 "Dokumenty", §9): which picked files are turned
 * away before anything is read, what a new document is called, and the sub-line of a row. Pure, so
 * the camera, the picker and the section share one answer.
 */
import type { AttachmentContent } from '@shared/schema';
import type { Formatters } from '@/lib/format';
import { fileSizeText } from '@/utils/file-size';
import { attachmentView, type AttachmentNote } from '@/utils/attachment-content';
import { viewerKind } from '@/utils/attachment-viewer';

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

/** What a document row's sub-line reads. */
type DocumentSubInput = {
  fileType: string;
  fileSize: number | null;
  /** A real instant (the backend stamps it with the clock), not a calendar day. */
  uploadedAt: number;
} & (
  | { content?: undefined; mimeType?: string }
  // Where the content is with device sync always comes with the file's type: a file the phone
  // cannot show is never downloaded on open, so its row must not say "tap to download".
  | { content: AttachmentContent; mimeType: string }
);

/**
 * The words a row's note can take: `notHereSub` is the phone's own (`mobile.documents`), the rest
 * are `common.attachments.content.*`. The caller resolves the text.
 */
export type DocumentNoteKey = 'notHereSub' | 'elsewhereSub' | AttachmentNote;

/** The note a row's sub-line starts with, or `null` for content that is here. */
function documentNoteKey(content: AttachmentContent, mimeType: string): DocumentNoteKey | null {
  // The phone's `notHere` is a document to download, not a spinner (`attachmentView`'s `download`
  // action)
  if (content === 'notHere') {
    // A file the phone cannot show is not downloaded on open (no share action here), so no "tap to
    // download"
    return viewerKind(mimeType) === 'external' ? 'elsewhereSub' : 'notHereSub';
  }
  return attachmentView(content, 'phone').note;
}

/**
 * The sub-line of a document row: the type's label · the size · the upload day ("smlouva · 1,2 MB ·
 * 24. 10. 2026"). The size is left out when it is not known. The day is the local one — the upload
 * is a real instant, so a document added at 00:30 is not dated the day before. A document whose
 * content is not on the phone starts with the note that says where it is (`noteText` words it;
 * first, so a narrow screen's cut falls on the day); content that is here (or sync off) gives the
 * line it always was.
 */
export function documentSub(
  doc: DocumentSubInput,
  typeLabel: (fileType: string) => string,
  fmt: Pick<Formatters, 'number' | 'date'>,
  noteText?: (key: DocumentNoteKey) => string
): string {
  const noteKey = doc.content && noteText ? documentNoteKey(doc.content, doc.mimeType) : null;
  return [
    noteKey && noteText ? noteText(noteKey) : '',
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
