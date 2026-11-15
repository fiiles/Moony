/**
 * What the phone's viewers do for a document or photo, given where its content is (ADR 0030;
 * `attachmentView(…, 'phone')` is the table of the notes). The phone downloads full content only
 * when it is opened: these are the decisions of that open, pure, so the document viewer and the
 * photo viewer share them.
 */
import type { AttachmentContent } from '@shared/schema';
import type { AttachmentNote } from '@/utils/attachment-content';

/** What a viewer draws: the content, a spinner while it comes, or why there is nothing to draw. */
export type FetchPlan =
  | { stage: 'show' }
  | { stage: 'fetching' }
  | { stage: 'unavailable'; note: AttachmentNote; retry: TryAgainAction | null };

/** "Try again": ask for content that is not here, or reset a transfer that failed. */
export type TryAgainAction = 'download' | 'retry';

/**
 * `askFailed`: this open's ask for the content was rejected, so the row is still `notHere` and no
 * spinner may wait for something that is not coming. It matters for `notHere` only.
 */
export function fetchPlan(content: AttachmentContent, askFailed: boolean): FetchPlan {
  switch (content) {
    case 'here':
    case 'waitingUpload':
      return { stage: 'show' };
    case 'notHere':
      return askFailed
        ? { stage: 'unavailable', note: 'failed', retry: 'download' }
        : { stage: 'fetching' };
    case 'downloading':
      return { stage: 'fetching' };
    case 'failed':
      return { stage: 'unavailable', note: 'failed', retry: 'retry' };
    case 'notOnServer':
      return { stage: 'unavailable', note: 'notOnServer', retry: null };
    case 'missing':
      return { stage: 'unavailable', note: 'missing', retry: null };
    default: {
      const unreachable: never = content;
      return unreachable;
    }
  }
}

/** One ask per open, for content that is not here and not already asked for. */
export function shouldAskDownload(content: AttachmentContent, alreadyAsked: boolean): boolean {
  return content === 'notHere' && !alreadyAsked;
}

/** What "Try again" does for a row in this state, `null` when it has nothing to try. */
export function tryAgainAction(content: AttachmentContent): TryAgainAction | null {
  if (content === 'notHere') return 'download';
  if (content === 'failed') return 'retry';
  return null;
}

/** What a viewer layers: the loading skeleton and its text, the thumbnail, the note card. */
export interface ViewerLayers {
  skeleton: boolean;
  thumbnail: boolean;
  note: boolean;
}

/** The loading skeleton belongs to a full load that is really under way, never to a state's note card. */
export function viewerLayers(
  stage: FetchPlan['stage'],
  status: 'loading' | 'ready' | 'failed' | 'fetching'
): ViewerLayers {
  return {
    skeleton: stage === 'show' && status === 'loading',
    thumbnail: stage !== 'show',
    note: stage !== 'show',
  };
}

/**
 * Whether tapping a row opens the viewer. Deliberately differs from `attachmentView(…, 'phone')
 * .openable` for `failed` and `downloading`: a failed row opens to "Try again", a downloading one
 * to the spinner. Everything does but content that cannot arrive
 * (`notOnServer`: the other device has not uploaded it) or is lost (`missing`): the row's own note
 * says so. A failed or downloading one opens to its state, with Try again for a failed one.
 */
export function rowOpensViewer(content: AttachmentContent): boolean {
  return content !== 'notOnServer' && content !== 'missing';
}
