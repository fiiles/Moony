import { describe, expect, it } from 'vitest';
import type { AttachmentStatus } from '@shared/schema';
import { attachmentStatusLines } from './attachment-status';

const base: AttachmentStatus = {
  storeAvailable: true,
  stored: 0,
  legacyPending: 0,
  missing: 0,
  failed: 0,
  converting: false,
  done: 0,
  total: 0,
  leftovers: [],
};
const keys = (status: Partial<AttachmentStatus>) =>
  attachmentStatusLines({ ...base, ...status }).map((line) => line.key);

describe('attachmentStatusLines', () => {
  it('says there are none yet on an empty store', () => {
    expect(keys({})).toEqual(['data.attachments.none']);
  });

  it('counts the stored attachments', () => {
    expect(attachmentStatusLines({ ...base, stored: 3 })).toEqual([
      { key: 'data.attachments.stored', params: { n: 3 } },
    ]);
  });

  it('warns first when the store cannot be opened, whatever else it says', () => {
    const lines = attachmentStatusLines({ ...base, storeAvailable: false, stored: 3, failed: 1 });
    expect(lines.map((line) => [line.key, line.tone])).toEqual([
      ['data.attachments.unavailable', 'loss'],
      ['data.attachments.failed', 'loss'],
    ]);
  });

  it('follows a running conversion and leaves the waiting count to it', () => {
    expect(
      attachmentStatusLines({ ...base, converting: true, done: 2, total: 5, legacyPending: 3 })
    ).toEqual([{ key: 'data.attachments.converting', params: { done: 2, total: 5 } }]);
  });

  it('lists plain files waiting, failures and missing files after the store line', () => {
    expect(keys({ stored: 4, legacyPending: 2, failed: 1, missing: 1 })).toEqual([
      'data.attachments.stored',
      'data.attachments.pending',
      'data.attachments.failed',
      'data.attachments.missing',
    ]);
  });

  it('does not claim "none" while plain files still wait', () => {
    expect(keys({ legacyPending: 2 })).toEqual(['data.attachments.pending']);
  });
});
