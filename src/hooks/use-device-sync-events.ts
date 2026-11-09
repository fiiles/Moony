import { useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import type { SyncStatus } from '@shared/schema';
import { portfolioApi } from '@/lib/tauri-api';
import { refreshAfterRemoteChanges } from '@/lib/device-sync-refresh';

/** Emitted by the sync engine whenever the status differs from the last one sent. */
export const SYNC_STATUS_EVENT = 'sync-status-changed';
/** Emitted after a round merged other devices' changes or restored a snapshot. */
export const SYNC_DATA_EVENT = 'sync-data-changed';

/** Mounted once in the app shell: keeps the status cache live and refreshes after remote changes. */
export function useDeviceSyncEvents(): void {
  const queryClient = useQueryClient();

  useEffect(() => {
    const status = listen<SyncStatus>(SYNC_STATUS_EVENT, (event) => {
      queryClient.setQueryData(['device-sync-status'], event.payload);
    });
    const data = listen(SYNC_DATA_EVENT, () => {
      void refreshAfterRemoteChanges(queryClient, portfolioApi.recordSnapshot);
    });

    return () => {
      void status.then((unlisten) => unlisten());
      void data.then((unlisten) => unlisten());
    };
  }, [queryClient]);
}
