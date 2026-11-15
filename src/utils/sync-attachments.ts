import type { SyncAttachments, SyncAttachmentsProblem, SyncStatus } from '@shared/schema';

/** The "Documents and photos" line of the sync status (attachments design §5.6). */
export type AttachmentsLine =
  | { kind: 'allUploaded' }
  | { kind: 'waiting'; count: number; bytes: number }
  | { kind: 'problem'; problem: SyncAttachmentsProblem };

/** A problem wins while anything waits to upload; else what waits, with its size; else all uploaded. */
export function attachmentsLine(a: SyncAttachments): AttachmentsLine {
  if (a.waitingUpload > 0 && a.problem) return { kind: 'problem', problem: a.problem };
  if (a.waitingUpload > 0) {
    return { kind: 'waiting', count: a.waitingUpload, bytes: a.waitingUploadBytes };
  }
  return { kind: 'allUploaded' };
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
