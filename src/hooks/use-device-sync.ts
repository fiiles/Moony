import { useQuery } from '@tanstack/react-query';
import { deviceSyncApi } from '@/lib/tauri-api';

/** Whether device sync is built in and enabled; `false` while loading or when asking fails. */
export function useDeviceSyncFeature(): boolean {
  const { data } = useQuery({
    queryKey: ['device-sync-feature'],
    queryFn: () => deviceSyncApi.featureEnabled(),
    staleTime: Infinity,
  });
  return data === true;
}

/** The sync status; the backend pushes changes through `sync-status-changed` (see the events hook). */
export function useDeviceSyncStatus() {
  const enabled = useDeviceSyncFeature();
  return useQuery({
    queryKey: ['device-sync-status'],
    queryFn: () => deviceSyncApi.getStatus(),
    enabled,
  });
}

/**
 * The sync account (number, and whether the subscription is active). `pollWhileInactive`
 * re-reads it every 3 s until it is active: setup waits for the payment this way.
 */
export function useDeviceSyncAccount(opts: { pollWhileInactive?: boolean } = {}) {
  const enabled = useDeviceSyncFeature();
  return useQuery({
    queryKey: ['device-sync-account'],
    queryFn: () => deviceSyncApi.getAccount(),
    enabled,
    refetchInterval: (query) =>
      opts.pollWhileInactive && query.state.data?.active !== true ? 3000 : false,
  });
}
