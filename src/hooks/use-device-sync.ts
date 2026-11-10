import { useQuery } from '@tanstack/react-query';
import { deviceSyncApi } from '@/lib/tauri-api';

/**
 * Whether device sync is built in and enabled, and whether that is still being asked (a screen
 * that leaves when the flag is off must not leave before the answer is in).
 */
export function useDeviceSyncFeatureState(): { enabled: boolean; loading: boolean } {
  const { data, isPending } = useQuery({
    queryKey: ['device-sync-feature'],
    queryFn: () => deviceSyncApi.featureEnabled(),
    staleTime: Infinity,
  });
  return { enabled: data === true, loading: isPending };
}

/** Whether device sync is built in and enabled; `false` while loading or when asking fails. */
export function useDeviceSyncFeature(): boolean {
  return useDeviceSyncFeatureState().enabled;
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

/** The devices of the sync account (this one marked); read while sync is on. */
export function useSyncDevices() {
  const enabled = useDeviceSyncFeature();
  const { data: status } = useDeviceSyncStatus();
  const syncOn = status !== undefined && status.phase !== 'off' && status.phase !== 'settingUp';
  return useQuery({
    queryKey: ['device-sync-devices'],
    queryFn: () => deviceSyncApi.listDevices(),
    enabled: enabled && syncOn,
  });
}
