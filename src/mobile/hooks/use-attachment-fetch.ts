import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { AttachmentContent, AttachmentKind } from '@shared/schema';
import { deviceSyncApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import {
  fetchPlan,
  shouldAskDownload,
  tryAgainAction,
  type FetchPlan,
} from '@/mobile/lib/attachment-fetch';

interface AttachmentFetch {
  plan: FetchPlan;
  /** "Try again" is in flight: the button is busy and a second tap is ignored. */
  retrying: boolean;
  tryAgain: () => void;
}

/**
 * What a phone viewer does while it is open for a document or photo (design spec "phone": full
 * content is downloaded only when it is opened). Asks the engine for the content ONCE per opening,
 * and only when the row is `notHere`; never on a render or a refetch, never for content that is
 * here. When the ask is rejected the viewer shows `failed` ("Try again") instead of a spinner that
 * would never end. The list refreshes through the `sync-attachments-changed` event, so the row's
 * `content` moves on by itself: nothing is invalidated by hand, and these calls change no financial
 * data, so no snapshot.
 *
 * `skip` is for a viewer that has nothing to show for this content anyway (a file type the phone
 * cannot display): it asks for nothing and its plan is always `show`.
 */
export function useAttachmentFetch(args: {
  open: boolean;
  kind: AttachmentKind;
  id: string | undefined;
  content: AttachmentContent | undefined;
  skip?: boolean;
}): AttachmentFetch {
  const { open, kind, id, content, skip = false } = args;
  const { t: tc } = useTranslation('common');
  const [askFailed, setAskFailed] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const retryingRef = useRef(false);
  // The id this opening already asked for
  const asked = useRef<string | null>(null);
  // The newest ask: a result of an earlier one (an earlier opening, a superseded retry) is ignored
  const seq = useRef(0);

  // Every opening of an item starts without a failed ask
  const visit = open ? (id ?? null) : null;
  const [seenVisit, setSeenVisit] = useState(visit);
  if (seenVisit !== visit) {
    setSeenVisit(visit);
    setAskFailed(false);
    setRetrying(false);
  }

  useEffect(() => {
    if (!open) {
      asked.current = null;
      seq.current += 1;
      // A retry still in flight no longer blocks the next opening
      retryingRef.current = false;
      return;
    }
    if (skip || !id || !content) return;
    if (!shouldAskDownload(content, asked.current === id)) return;
    asked.current = id;
    const token = ++seq.current;
    // The event refreshes the lists; nothing is invalidated here and no snapshot is recorded
    deviceSyncApi.downloadAttachment(kind, id).catch(() => {
      if (token === seq.current) setAskFailed(true);
    });
  }, [open, kind, id, content, skip]);

  const tryAgain = () => {
    if (!id || !content || retryingRef.current) return;
    const action = tryAgainAction(content);
    if (!action) return;
    retryingRef.current = true;
    setRetrying(true);
    setAskFailed(false);
    asked.current = id;
    const token = ++seq.current;
    // As the first ask: the event refreshes the row, no cache write by hand, no snapshot
    const call =
      action === 'download'
        ? deviceSyncApi.downloadAttachment(kind, id)
        : deviceSyncApi.retryAttachment(kind, id);
    call
      .catch((error: unknown) => {
        if (token !== seq.current) return;
        if (action === 'download') setAskFailed(true);
        else {
          toast.error(tc('status.error'), {
            description: translateApiError(error instanceof Error ? error : String(error), tc),
          });
        }
      })
      .finally(() => {
        if (token !== seq.current) return;
        retryingRef.current = false;
        setRetrying(false);
      });
  };

  const plan: FetchPlan = skip || !content ? { stage: 'show' } : fetchPlan(content, askFailed);
  return { plan, retrying, tryAgain };
}
