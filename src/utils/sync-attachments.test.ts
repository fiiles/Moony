import { describe, expect, it } from 'vitest';
import type { SyncAttachments, SyncPhase, SyncStatus } from '@shared/schema';
import {
  attachmentsLine,
  failedUploadLine,
  notInBackup,
  offerDownloadEverything,
  onOtherDevices,
  shownProblem,
  uploadsKeepUnsynced,
  withoutDanglingSize,
} from './sync-attachments';

const zeros: SyncAttachments = {
  waitingUpload: 0,
  waitingUploadBytes: 0,
  notHere: 0,
  notOnServer: 0,
  failed: 0,
  notOnDevice: 0,
  failedUpload: 0,
  problem: null,
};

const attachments = (over: Partial<SyncAttachments>): SyncAttachments => ({ ...zeros, ...over });

describe('shownProblem', () => {
  it('shows noRoom only while something waits to upload', () => {
    expect(shownProblem(attachments({ problem: 'noRoom' }))).toBeNull();
    expect(shownProblem(attachments({ problem: 'noRoom', waitingUpload: 2 }))).toBe('noRoom');
  });
  it('shows serverNeedsUpdate whenever it is set', () => {
    expect(shownProblem(attachments({ problem: 'serverNeedsUpdate' }))).toBe('serverNeedsUpdate');
    expect(shownProblem(attachments({ problem: 'serverNeedsUpdate', waitingUpload: 3 }))).toBe(
      'serverNeedsUpdate'
    );
  });
  it('is none without a problem', () => {
    expect(shownProblem(zeros)).toBeNull();
    expect(shownProblem(attachments({ waitingUpload: 1 }))).toBeNull();
  });
});

describe('attachmentsLine', () => {
  it('reads all uploaded on zeros, whatever is missing here', () => {
    expect(attachmentsLine(zeros)).toEqual({ kind: 'allUploaded' });
    expect(attachmentsLine(attachments({ notHere: 5, notOnServer: 2, failed: 1 }))).toEqual({
      kind: 'allUploaded',
    });
  });

  it('counts what waits with its size, also beside a problem', () => {
    const waiting = { kind: 'waiting', count: 3, bytes: 12_000_000 };
    expect(
      attachmentsLine(attachments({ waitingUpload: 3, waitingUploadBytes: 12_000_000 }))
    ).toEqual(waiting);
    expect(
      attachmentsLine(
        attachments({ waitingUpload: 3, waitingUploadBytes: 12_000_000, problem: 'noRoom' })
      )
    ).toEqual(waiting);
    expect(
      attachmentsLine(
        attachments({
          waitingUpload: 3,
          waitingUploadBytes: 12_000_000,
          problem: 'serverNeedsUpdate',
        })
      )
    ).toEqual(waiting);
  });

  it('never says all uploaded while a problem is shown', () => {
    expect(attachmentsLine(attachments({ problem: 'serverNeedsUpdate' }))).toEqual({
      kind: 'none',
    });
  });

  it('treats noRoom with nothing waiting as no problem', () => {
    expect(attachmentsLine(attachments({ problem: 'noRoom' }))).toEqual({ kind: 'allUploaded' });
  });
});

describe('withoutDanglingSize', () => {
  it('drops the separator left by an empty size', () => {
    expect(withoutDanglingSize('3 waiting to upload · ')).toBe('3 waiting to upload');
    expect(withoutDanglingSize('3 waiting to upload · 12.0 MB')).toBe(
      '3 waiting to upload · 12.0 MB'
    );
  });
});

describe('the waiting strings of every locale', () => {
  const files = import.meta.glob<{ attachments: Record<string, string> }>(
    '../i18n/locales/*/deviceSync.json',
    { eager: true, import: 'default' }
  );
  it('end with the size, so an empty size leaves only a separator to strip', () => {
    expect(Object.keys(files)).toHaveLength(10);
    for (const [path, tree] of Object.entries(files)) {
      const forms = Object.entries(tree.attachments).filter(([k]) => k.startsWith('waiting_'));
      expect(forms.length, path).toBeGreaterThan(0);
      for (const [key, text] of forms) expect(text, `${path} ${key}`).toMatch(/ · \{\{size\}\}$/);
    }
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

describe('failedUploadLine', () => {
  it('is the count of stuck uploads, null when there are none', () => {
    expect(failedUploadLine(zeros)).toBeNull();
    expect(failedUploadLine(attachments({ failed: 3 }))).toBeNull();
    expect(failedUploadLine(attachments({ failed: 3, failedUpload: 2 }))).toBe(2);
  });
  it('never lets the row read all uploaded while one is stuck', () => {
    expect(attachmentsLine(attachments({ failed: 1, failedUpload: 1 }))).toEqual({ kind: 'none' });
  });
  it('keeps the waiting text as the value beside a stuck upload', () => {
    expect(
      attachmentsLine(
        attachments({ waitingUpload: 2, waitingUploadBytes: 500, failed: 1, failedUpload: 1 })
      )
    ).toEqual({ kind: 'waiting', count: 2, bytes: 500 });
  });
});
