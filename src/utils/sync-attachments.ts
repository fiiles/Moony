import type { SyncAttachments, SyncAttachmentsProblem, SyncStatus } from '@shared/schema';

/**
 * The attachments problem the person is told about. The engine keeps it as policy state that does
 * not clear when the queue empties, so `noRoom` counts only while something waits to upload (with
 * nothing waiting there is nothing it holds back); `serverNeedsUpdate` counts whenever it is set.
 * Every surface reads the problem through this, never from `SyncAttachments.problem`.
 */
export function shownProblem(a: SyncAttachments): SyncAttachmentsProblem | null {
  if (a.problem === 'noRoom') return a.waitingUpload > 0 ? 'noRoom' : null;
  return a.problem;
}

/** What the value of the "Documents and photos" row says. */
export type AttachmentsLine =
  | { kind: 'allUploaded' }
  | { kind: 'waiting'; count: number; bytes: number }
  /** A problem is shown and nothing waits: the row has no value, the problem line stands alone. */
  | { kind: 'none' };

/** What waits, with its size; else all uploaded, but never while a problem is shown (then nothing). */
export function attachmentsLine(a: SyncAttachments): AttachmentsLine {
  if (a.waitingUpload > 0) {
    return { kind: 'waiting', count: a.waitingUpload, bytes: a.waitingUploadBytes };
  }
  return shownProblem(a) ? { kind: 'none' } : { kind: 'allUploaded' };
}

/**
 * "3 waiting to upload · " (the size is unknown, so empty) loses its separator. Every locale's
 * `attachments.waiting_*` ends with " · {{size}}" (a test holds them to it).
 */
export function withoutDanglingSize(text: string): string {
  return text.replace(/ · $/, '');
}

/**
 * A device is synchronised only when nothing waits to upload (§5.6): the phase is idle but content
 * still waits. Stuck uploads are counted as `failed`, not `waitingUpload`, so they never keep it
 * "uploading" for good.
 */
export function uploadsKeepUnsynced(status: SyncStatus): boolean {
  return status.phase === 'idle' && status.attachments.waitingUpload > 0;
}

/**
 * Documents and photos that are on the person's other devices (fetchable, or not uploaded from
 * there yet). Failed rows have their own state and are not promised.
 */
export function onOtherDevices(a: SyncAttachments): number {
  return a.notHere + a.notOnServer;
}

/** Whether "Download everything" has something to fetch now (phone only; callers decide the platform). */
export function offerDownloadEverything(a: SyncAttachments): boolean {
  return a.notHere > 0;
}

/** Rows whose content a backup made on this device would not hold, whatever the reason. */
export function notInBackup(a: SyncAttachments | undefined): number {
  return a?.notOnDevice ?? 0;
}
