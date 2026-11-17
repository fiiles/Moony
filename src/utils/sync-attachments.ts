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

/**
 * What waits to upload, with its size; `null` when nothing does. Documents and photos are data like
 * any other: there is no row of their own, and with nothing to say no line is shown (the one sync
 * status covers them). The line stands beside a problem or a stuck upload.
 */
export function waitingUploadLine(a: SyncAttachments): { count: number; bytes: number } | null {
  return a.waitingUpload > 0 ? { count: a.waitingUpload, bytes: a.waitingUploadBytes } : null;
}

/**
 * How many documents and photos have an upload that cannot go by itself (refused, conflict,
 * invalid ID), `null` when none. Its own line under the sync status, beside what waits.
 */
export function failedUploadLine(a: SyncAttachments): number | null {
  return a.failedUpload > 0 ? a.failedUpload : null;
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

/**
 * Whether "Download everything" has something to fetch now (phone only; callers decide the
 * platform).
 */
export function offerDownloadEverything(a: SyncAttachments): boolean {
  return a.notHere > 0;
}

/** Rows whose content a backup made on this device would not hold, whatever the reason. */
export function notInBackup(a: SyncAttachments | undefined): number {
  return a?.notOnDevice ?? 0;
}
