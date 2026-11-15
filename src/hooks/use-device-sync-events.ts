import { useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { SyncStatus } from '@shared/schema';
import { portfolioApi } from '@/lib/tauri-api';
import { refreshAfterRemoteChanges } from '@/lib/device-sync-refresh';
import { useDeviceSyncFeature } from '@/hooks/use-device-sync';

/** Emitted by the sync engine whenever the status differs from the last one sent. */
export const SYNC_STATUS_EVENT = 'sync-status-changed';
/** Emitted after a round merged other devices' changes or restored a snapshot. */
export const SYNC_DATA_EVENT = 'sync-data-changed';

/**
 * Emitted (at most once per 250 ms) when documents or photos moved or their transfer state
 * changed: the lists that show where each one is refresh.
 */
export const SYNC_ATTACHMENTS_EVENT = 'sync-attachments-changed';

/** Query-key prefixes of the document and photo lists (desktop and phone use the same keys). */
const ATTACHMENT_LIST_KEYS = [
  ['insurance-documents'],
  ['real-estate-documents'],
  ['real-estate-photo-batches'],
] as const;

/** A rejected `listen` leaves nothing to unlisten; it must not go unhandled. */
function safeUnlisten(pending: Promise<UnlistenFn>): () => void {
  const settled = pending.then(
    (unlisten) => unlisten,
    () => undefined
  );
  return () => void settled.then((unlisten) => unlisten?.());
}

/**
 * Mounted once in the app shell: keeps the status cache live and refreshes after remote changes.
 * Listens only while the feature flag is on.
 */
export function useDeviceSyncEvents(): void {
  const queryClient = useQueryClient();
  const enabled = useDeviceSyncFeature();

  useEffect(() => {
    if (!enabled) return;
    const status = safeUnlisten(
      listen<SyncStatus>(SYNC_STATUS_EVENT, (event) => {
        queryClient.setQueryData(['device-sync-status'], event.payload);
        // A gap that appears, heals or ages shows with its held changes
        void queryClient.invalidateQueries({ queryKey: ['device-sync-held-gaps'] });
        // A stalled sync is a reminder, so the list follows the status
        void queryClient.invalidateQueries({ queryKey: ['milestones'] });
      })
    );
    const data = safeUnlisten(
      listen(SYNC_DATA_EVENT, () => {
        refreshAfterRemoteChanges(queryClient, portfolioApi.recordSnapshot).catch(() => {});
      })
    );
    const attachments = safeUnlisten(
      listen(SYNC_ATTACHMENTS_EVENT, () => {
        for (const queryKey of ATTACHMENT_LIST_KEYS) {
          void queryClient.invalidateQueries({ queryKey });
        }
        // The summary of what waits to move is part of the status
        void queryClient.invalidateQueries({ queryKey: ['device-sync-status'] });
      })
    );

    return () => {
      status();
      data();
      attachments();
    };
  }, [queryClient, enabled]);
}
