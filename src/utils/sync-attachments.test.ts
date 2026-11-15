import { describe, expect, it } from 'vitest';
import type { SyncAttachments, SyncPhase, SyncStatus } from '@shared/schema';
import {
  attachmentsLine,
  notInBackup,
  offerDownloadEverything,
  onOtherDevices,
  uploadsKeepUnsynced,
} from './sync-attachments';

const zeros: SyncAttachments = {
  waitingUpload: 0,
  waitingUploadBytes: 0,
  notHere: 0,
  notOnServer: 0,
  failed: 0,
  notOnDevice: 0,
  problem: null,
};

const attachments = (over: Partial<SyncAttachments>): SyncAttachments => ({ ...zeros, ...over });

describe('attachmentsLine', () => {
  it('reads all uploaded on zeros, whatever is missing here', () => {
    expect(attachmentsLine(zeros)).toEqual({ kind: 'allUploaded' });
    expect(attachmentsLine(attachments({ notHere: 5, notOnServer: 2, failed: 1 }))).toEqual({
      kind: 'allUploaded',
    });
  });

  it('counts what waits with its size', () => {
    expect(
      attachmentsLine(attachments({ waitingUpload: 3, waitingUploadBytes: 12_000_000 }))
    ).toEqual({ kind: 'waiting', count: 3, bytes: 12_000_000 });
  });

  it('lets a problem win while anything waits, else it does not change the line', () => {
    expect(attachmentsLine(attachments({ waitingUpload: 2, problem: 'noRoom' }))).toEqual({
      kind: 'problem',
      problem: 'noRoom',
    });
    expect(
      attachmentsLine(attachments({ waitingUpload: 1, problem: 'serverNeedsUpdate' }))
    ).toEqual({
      kind: 'problem',
      problem: 'serverNeedsUpdate',
    });
    expect(attachmentsLine(attachments({ problem: 'noRoom' }))).toEqual({ kind: 'allUploaded' });
  });
});

describe('uploadsKeepUnsynced', () => {
  const status = (phase: SyncPhase, over: Partial<SyncAttachments>) =>
    ({ phase, attachments: attachments(over) }) as SyncStatus;

  it('is true only for an idle phase with uploads waiting', () => {
    expect(uploadsKeepUnsynced(status('idle', { waitingUpload: 1 }))).toBe(true);
    expect(uploadsKeepUnsynced(status('idle', {}))).toBe(false);
    expect(uploadsKeepUnsynced(status('idle', { failed: 3, notHere: 2, notOnDevice: 5 }))).toBe(
      false
    );
    expect(uploadsKeepUnsynced(status('syncing', { waitingUpload: 1 }))).toBe(false);
    expect(uploadsKeepUnsynced(status('paused', { waitingUpload: 1 }))).toBe(false);
    expect(uploadsKeepUnsynced(status('off', { waitingUpload: 1 }))).toBe(false);
  });

  it('keeps counting uploads whatever the attachments problem is', () => {
    expect(uploadsKeepUnsynced(status('idle', { waitingUpload: 2, problem: 'noRoom' }))).toBe(true);
    expect(uploadsKeepUnsynced(status('idle', { problem: 'noRoom' }))).toBe(false);
  });
});

describe('offerDownloadEverything', () => {
  it('offers only while a row can be fetched', () => {
    expect(offerDownloadEverything(zeros)).toBe(false);
    expect(offerDownloadEverything(attachments({ notHere: 1 }))).toBe(true);
    // not on the server, failed: nothing the button could fetch now
    expect(
      offerDownloadEverything(attachments({ notOnServer: 3, failed: 2, notOnDevice: 5 }))
    ).toBe(false);
    expect(offerDownloadEverything(attachments({ notHere: 1, notOnServer: 3 }))).toBe(true);
  });
});

describe('onOtherDevices', () => {
  it('adds the fetchable and the not-yet-uploaded rows, never the failed ones', () => {
    expect(onOtherDevices(zeros)).toBe(0);
    expect(onOtherDevices(attachments({ notHere: 3, notOnServer: 2, failed: 4 }))).toBe(5);
    expect(onOtherDevices(attachments({ failed: 1, notOnDevice: 1 }))).toBe(0);
  });
});

describe('notInBackup', () => {
  it('counts every row not complete on this device', () => {
    expect(notInBackup(undefined)).toBe(0);
    expect(notInBackup(zeros)).toBe(0);
    expect(
      notInBackup(attachments({ notHere: 3, notOnServer: 2, failed: 1, notOnDevice: 6 }))
    ).toBe(6);
    // a stuck upload is failed but its content is here: not missing from a backup
    expect(notInBackup(attachments({ failed: 1 }))).toBe(0);
  });
});
