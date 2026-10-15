/**
 * Attachment bytes reach the webview only through the `moony-file` scheme (ADR 0017), addressed
 * by the owning row: `<kind>/<row id>`. Storage paths never reach the frontend.
 */
import { convertFileSrc } from '@tauri-apps/api/core';
import type { AttachmentKind } from '@shared/schema';

/** What a `moony-file` URL can name: an attachment kind, or a photo's thumbnail. */
export type AttachmentRoute = AttachmentKind | 'photo-thumb';

export const ATTACHMENT_SCHEME = 'moony-file';

/** The scheme path of an attachment, as the backend's route parser expects it. */
export function attachmentPath(route: AttachmentRoute, id: string): string {
  return `${route}/${id}`;
}

/** The URL an `<img>` or a `fetch` uses for an attachment, in this platform's form. */
export function attachmentUrl(route: AttachmentRoute, id: string): string {
  return convertFileSrc(attachmentPath(route, id), ATTACHMENT_SCHEME);
}
