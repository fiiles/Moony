import type { AttachmentStatus } from '@shared/schema';

/** The status query's key: the desktop's Data card and the phone's Záloha a přenos read one entry. */
export const ATTACHMENT_STATUS_KEY = ['attachment-status'] as const;

/** One line of the attachments status: a key of `settings` with its values, `loss` when it warns. */
export interface AttachmentStatusLine {
  key: string;
  params: Record<string, number>;
  tone?: 'loss';
}

/**
 * The lines that say where the encrypted attachments stand (encrypted-attachments spec §4.8,
 * §4.11), in the order the desktop's Data card and the phone's Záloha a přenos show them: the store
 * (unavailable, converting, how many are stored, or none yet), then plain files still waiting, files
 * the last run could not convert, and files missing on disk. The leftovers outside the app's folder
 * are their own block (`status.leftovers`).
 */
export function attachmentStatusLines(status: AttachmentStatus): AttachmentStatusLine[] {
  const lines: AttachmentStatusLine[] = [];
  if (!status.storeAvailable) {
    lines.push({ key: 'data.attachments.unavailable', params: {}, tone: 'loss' });
  } else if (status.converting) {
    lines.push({
      key: 'data.attachments.converting',
      params: { done: status.done, total: status.total },
    });
  } else if (status.stored > 0) {
    lines.push({ key: 'data.attachments.stored', params: { n: status.stored } });
  } else if (status.legacyPending === 0) {
    lines.push({ key: 'data.attachments.none', params: {} });
  }
  if (!status.converting && status.legacyPending > 0) {
    lines.push({ key: 'data.attachments.pending', params: { n: status.legacyPending } });
  }
  if (status.failed > 0) {
    lines.push({ key: 'data.attachments.failed', params: { n: status.failed }, tone: 'loss' });
  }
  if (status.missing > 0) {
    lines.push({ key: 'data.attachments.missing', params: { n: status.missing } });
  }
  return lines;
}
