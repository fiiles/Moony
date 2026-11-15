/**
 * What a person sees and can do for a document or photo, given where its content is with device
 * sync (ADR 0030; design spec `2026-11-14-sync-attachments-design.md`). Pure, so both the desktop
 * tables and the phone share one table. Without sync every row is `here`, which is plain and
 * openable, with no note.
 */
import type { AttachmentContent } from '@shared/schema';

export type AttachmentPlatform = 'desktop' | 'phone';

/** The key under `common.attachments.content` that words the state. */
export type AttachmentNote = 'waitingUpload' | 'downloading' | 'notOnServer' | 'failed' | 'missing';

export interface AttachmentView {
  /** The content can be opened now (or, on the phone, opening downloads it). */
  openable: boolean;
  note: AttachmentNote | null;
  /** The button the row offers: fetch it (phone) or try again after a failure. */
  action: 'download' | 'retry' | null;
  /** Something is moving: show a spinner, offer no button. */
  busy: boolean;
}

/**
 * Every wire value, as a record keyed by `AttachmentContent`: a value the schema adds fails the
 * typecheck here until it is listed (and `attachmentView` below fails its `never` check).
 */
const ALL_CONTENT: Record<AttachmentContent, true> = {
  here: true,
  waitingUpload: true,
  notHere: true,
  downloading: true,
  notOnServer: true,
  failed: true,
  missing: true,
};
export const ATTACHMENT_CONTENT_VALUES = Object.keys(ALL_CONTENT) as AttachmentContent[];

export function attachmentView(
  content: AttachmentContent,
  platform: AttachmentPlatform
): AttachmentView {
  switch (content) {
    case 'here':
      return { openable: true, note: null, action: null, busy: false };
    case 'waitingUpload':
      return { openable: true, note: 'waitingUpload', action: null, busy: false };
    case 'notHere':
      // The desktop's lanes fetch everything by themselves; the phone fetches on open.
      return platform === 'desktop'
        ? { openable: false, note: 'downloading', action: null, busy: true }
        : { openable: true, note: null, action: 'download', busy: false };
    case 'downloading':
      return { openable: false, note: 'downloading', action: null, busy: true };
    case 'notOnServer':
      return { openable: false, note: 'notOnServer', action: null, busy: false };
    case 'failed':
      return { openable: false, note: 'failed', action: 'retry', busy: false };
    case 'missing':
      return { openable: false, note: 'missing', action: null, busy: false };
    default: {
      const unreachable: never = content;
      return unreachable;
    }
  }
}

/** A thumbnail can be drawn only from bytes on this device. */
export function thumbnailShown(content: AttachmentContent): boolean {
  return content === 'here' || content === 'waitingUpload';
}

/**
 * What fills a thumbnail's box when no image is drawn: a pulsing skeleton only while the bytes may
 * still arrive (`notHere`, `downloading`), a still tile when nothing is coming (failed, missing,
 * not on the server) or when the image itself would not load.
 */
export function thumbnailPlaceholder(content: AttachmentContent): 'loading' | 'static' {
  return content === 'notHere' || content === 'downloading' ? 'loading' : 'static';
}
