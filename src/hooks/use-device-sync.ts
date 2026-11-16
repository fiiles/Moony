import { useQuery } from '@tanstack/react-query';
import { deviceSyncApi } from '@/lib/tauri-api';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { syncIsOn } from '@/utils/safe-copy';
import { syncStatusRefetchInterval } from '@/utils/device-sync-refetch';

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

/** What this profile says about sync in any build (`sync_profile_state`). */
export function useSyncProfileState() {
  return useQuery({
    queryKey: ['device-sync-profile-state'],
    queryFn: () => deviceSyncApi.profileState(),
    staleTime: Infinity,
  });
}

/**
 * Whether this profile has sync on while this build does not run sync (a profile a development
 * build enabled): the one case where a way out is shown. `pending`: changes recorded but never
 * sent; `loading`: the answer is not in yet.
 */
export function useLeftoverSyncProfile(): { leftover: boolean; pending: number; loading: boolean } {
  const { data, isPending } = useSyncProfileState();
  return {
    leftover: data !== undefined && data.enabled && !data.featureOn,
    pending: data?.pendingChanges ?? 0,
    loading: isPending,
  };
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
    refetchInterval: (query) => syncStatusRefetchInterval(query.state.data),
  });
}

/**
 * The sync account (number, and whether the subscription is active). `pollWhileInactive`
 * re-reads it every 3 s until it is active: setup waits for the payment this way. `enabled: false`
 * keeps it from contacting the server at all (setup's server step).
 */
export function useDeviceSyncAccount(
  opts: { pollWhileInactive?: boolean; enabled?: boolean } = {}
) {
  const featureEnabled = useDeviceSyncFeature();
  const enabled = featureEnabled && opts.enabled !== false;
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
  const syncOn = syncIsOn(true, status);
  return useQuery({
    queryKey: ['device-sync-devices'],
    queryFn: () => deviceSyncApi.listDevices(),
    enabled: enabled && syncOn,
  });
}

/**
 * What a replace would put at stake now. Never cached: a confirmation reads it when it opens
 * (`enabled`) and again after `validation.syncReplaceImpactChanged`.
 */
export function useReplaceImpact(enabled = true) {
  return useQuery({
    queryKey: ['device-sync-replace-impact'],
    queryFn: () => deviceSyncApi.replaceImpact(),
    enabled,
    gcTime: 0,
    staleTime: 0,
    retry: false,
    refetchOnWindowFocus: false,
  });
}

/** The backups joins, repairs and re-syncs made, newest first; read while sync is built in. */
export function useSyncBackups() {
  const enabled = useDeviceSyncFeature();
  return useQuery({
    queryKey: ['device-sync-backups'],
    queryFn: () => deviceSyncApi.listBackups(),
    enabled,
  });
}

/**
 * The changes from other devices held because something they need never arrived (the `gap`
 * problem), the oldest first; read while `active` (the status shows a gap). The status event
 * re-reads it.
 */
export function useHeldGaps(active: boolean) {
  const enabled = useDeviceSyncFeature();
  return useQuery({
    queryKey: ['device-sync-held-gaps'],
    queryFn: () => deviceSyncApi.heldGaps(),
    enabled: enabled && active,
  });
}

/**
 * Asks the server at an address, or named by a connection code, what it is, once the input stopped
 * changing for 400 ms. Nothing is kept: the input may be a connection code.
 */
export function useSyncServerCheck(input: string, enabled: boolean) {
  const trimmed = useDebouncedValue(input.trim(), 400);
  return useQuery({
    queryKey: ['device-sync-server-check', trimmed],
    queryFn: () => deviceSyncApi.checkServer(trimmed),
    enabled: enabled && trimmed !== '',
    retry: false,
    staleTime: 0,
    gcTime: 0,
  });
}
