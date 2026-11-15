import { describe, expect, it } from 'vitest';
import type { AttachmentContent } from '@shared/schema';
import { ATTACHMENT_CONTENT_VALUES } from '@/utils/attachment-content';
import { fetchPlan, rowOpensViewer, shouldAskDownload, tryAgainAction } from './attachment-fetch';

describe('fetchPlan', () => {
  it('shows the content that is on the phone, waiting to upload included', () => {
    expect(fetchPlan('here', false)).toEqual({ stage: 'show' });
    expect(fetchPlan('waitingUpload', false)).toEqual({ stage: 'show' });
  });

  it('waits with a spinner while the content is on its way', () => {
    expect(fetchPlan('notHere', false)).toEqual({ stage: 'fetching' });
    expect(fetchPlan('downloading', false)).toEqual({ stage: 'fetching' });
  });

  it('turns a failed ask for content that is not here into the failed state with a way to ask again', () => {
    expect(fetchPlan('notHere', true)).toEqual({
      stage: 'unavailable',
      note: 'failed',
      retry: 'download',
    });
  });

  it('says why the rest cannot be shown, and offers Try again only for a failed row', () => {
    expect(fetchPlan('failed', false)).toEqual({
      stage: 'unavailable',
      note: 'failed',
      retry: 'retry',
    });
    expect(fetchPlan('notOnServer', false)).toEqual({
      stage: 'unavailable',
      note: 'notOnServer',
      retry: null,
    });
    expect(fetchPlan('missing', false)).toEqual({
      stage: 'unavailable',
      note: 'missing',
      retry: null,
    });
  });

  it('lets a failed ask matter only for content that is not here', () => {
    for (const content of ATTACHMENT_CONTENT_VALUES) {
      if (content === 'notHere') continue;
      expect(fetchPlan(content, true)).toEqual(fetchPlan(content, false));
    }
  });
});

describe('shouldAskDownload', () => {
  it('asks once, and only for content that is not here', () => {
    for (const content of ATTACHMENT_CONTENT_VALUES) {
      expect(shouldAskDownload(content, false)).toBe(content === 'notHere');
      expect(shouldAskDownload(content, true)).toBe(false);
    }
  });
});

describe('tryAgainAction', () => {
  it('asks again for a row that is still not here and retries a failed one', () => {
    expect(tryAgainAction('notHere')).toBe('download');
    expect(tryAgainAction('failed')).toBe('retry');
  });

  it('does nothing for the rest', () => {
    const rest: AttachmentContent[] = [
      'here',
      'waitingUpload',
      'downloading',
      'notOnServer',
      'missing',
    ];
    for (const content of rest) expect(tryAgainAction(content)).toBeNull();
  });
});

describe('rowOpensViewer', () => {
  it('opens for everything but content that cannot arrive or is lost', () => {
    for (const content of ATTACHMENT_CONTENT_VALUES) {
      expect(rowOpensViewer(content)).toBe(content !== 'notOnServer' && content !== 'missing');
    }
  });
});
