import { describe, expect, it } from 'vitest';
import type { AttachmentContent } from '@shared/schema';
import {
  ATTACHMENT_CONTENT_VALUES,
  attachmentView,
  thumbnailPlaceholder,
  thumbnailShown,
  type AttachmentPlatform,
  type AttachmentView,
} from './attachment-content';

const view = (
  openable: boolean,
  note: AttachmentView['note'],
  action: AttachmentView['action'],
  busy: boolean
): AttachmentView => ({ openable, note, action, busy });

const TABLE: Record<AttachmentContent, Record<AttachmentPlatform, AttachmentView>> = {
  here: {
    desktop: view(true, null, null, false),
    phone: view(true, null, null, false),
  },
  waitingUpload: {
    desktop: view(true, 'waitingUpload', null, false),
    phone: view(true, 'waitingUpload', null, false),
  },
  notHere: {
    desktop: view(false, 'downloading', null, true),
    phone: view(true, null, 'download', false),
  },
  downloading: {
    desktop: view(false, 'downloading', null, true),
    phone: view(false, 'downloading', null, true),
  },
  notOnServer: {
    desktop: view(false, 'notOnServer', null, false),
    phone: view(false, 'notOnServer', null, false),
  },
  failed: {
    desktop: view(false, 'failed', 'retry', false),
    phone: view(false, 'failed', 'retry', false),
  },
  missing: {
    desktop: view(false, 'missing', null, false),
    phone: view(false, 'missing', null, false),
  },
};

describe('attachmentView', () => {
  it('lists all seven wire values (the table above must cover each)', () => {
    expect([...ATTACHMENT_CONTENT_VALUES].sort()).toEqual(Object.keys(TABLE).sort());
  });

  for (const content of Object.keys(TABLE) as AttachmentContent[]) {
    for (const platform of ['desktop', 'phone'] as const) {
      it(`${content} on the ${platform}`, () => {
        expect(attachmentView(content, platform)).toEqual(TABLE[content][platform]);
      });
    }
  }
});

describe('thumbnailShown', () => {
  it('shows a thumbnail only when the bytes are on this device', () => {
    const expected: Record<AttachmentContent, boolean> = {
      here: true,
      waitingUpload: true,
      notHere: false,
      downloading: false,
      notOnServer: false,
      failed: false,
      missing: false,
    };
    for (const content of Object.keys(expected) as AttachmentContent[]) {
      expect(thumbnailShown(content), content).toBe(expected[content]);
    }
  });
});

describe('thumbnailPlaceholder', () => {
  it('pulses only while a thumbnail may still come', () => {
    const expected: Record<AttachmentContent, 'loading' | 'static'> = {
      here: 'static',
      waitingUpload: 'static',
      notHere: 'loading',
      downloading: 'loading',
      notOnServer: 'static',
      failed: 'static',
      missing: 'static',
    };
    for (const content of Object.keys(expected) as AttachmentContent[]) {
      expect(thumbnailPlaceholder(content), content).toBe(expected[content]);
    }
  });
});
