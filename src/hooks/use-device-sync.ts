import { useEffect, useRef, useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { deviceSyncApi } from '@/lib/tauri-api';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { syncIsOn } from '@/utils/safe-copy';
import { syncStatusRefetchInterval } from '@/utils/device-sync-refetch';
import { HOLDER_FINISH_LATE_MS, groupDevices, newlyJoinedDevice } from '@/utils/sync-devices';
import type { PairingView, SyncDevice } from '@shared/schema';

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

const DEVICES_KEY = ['device-sync-devices'] as const;

function useSyncDevicesQuery<T>(select: (devices: SyncDevice[]) => T) {
  const enabled = useDeviceSyncFeature();
  const { data: status } = useDeviceSyncStatus();
  const syncOn = syncIsOn(true, status);
  return useQuery({
    queryKey: DEVICES_KEY,
    queryFn: () => deviceSyncApi.listDevices(),
    select,
    enabled: enabled && syncOn,
  });
}

/**
 * The devices that are on the sync list now (this one marked): active and "Zkontrolujte". Former
 * devices (left or removed) are not among them; callers that count or name "other devices" want
 * this. The device list itself reads `useSyncDeviceGroups`.
 */
export function useSyncDevices() {
  return useSyncDevicesQuery((devices) => devices.filter((device) => device.status !== 'former'));
}

/** The device list in its three groups: active, "Zkontrolujte" and former (listed for 30 days). */
export function useSyncDeviceGroups() {
  return useSyncDevicesQuery((devices) => groupDevices(devices));
}

/**
 * What removing `deviceIds` (and the ticked `alsoRemove`) would do. Never cached: the consent in
 * the answer covers what the person was shown, so the dialog reads it when it opens and again
 * after a tick, "Je moje" or `validation.syncRemovalPlanChanged`. `newKit` asks for a new kit
 * with nobody removed (the new-kit dialog).
 */
export function useRemovalPlan(
  deviceIds: readonly string[],
  alsoRemove: readonly string[],
  newKit = false,
  enabled = true
) {
  // The first read of a dialog wakes a sync round (`refresh`); ticks, re-reads and the read after a
  // failed submit do not. The ref lives as long as the dialog is mounted (they mount only while open).
  const firstRead = useRef(true);
  return useQuery({
    queryKey: ['device-sync-removal-plan', deviceIds, alsoRemove, newKit],
    queryFn: () => {
      const refresh = firstRead.current;
      firstRead.current = false;
      return deviceSyncApi.removalPlan([...deviceIds], [...alsoRemove], newKit, refresh);
    },
    enabled,
    gcTime: 0,
    staleTime: 0,
    retry: false,
    refetchOnWindowFocus: false,
    // Ticking a box keeps the list on screen while the new plan loads
    placeholderData: keepPreviousData,
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

/** How often the holder's dialog re-reads the device list after it sent the sync details (ms). */
const HOLDER_FINISH_POLL_MS = 3000;

/**
 * The holder's finish (spec §5): while a pairing as the holder runs, the device list is read once
 * for what was there when it started; after `sent` it is watched (fast poll) for a device row
 * that was not. `joined` is that device's name once it appears; `late` turns true after two
 * minutes without it. Both reset with the pairing.
 */
export function useHolderFinish(view: PairingView | undefined): {
  joined: string | null;
  late: boolean;
} {
  const holding = view?.role === 'holder';
  const sent = holding && view.stage === 'sent';
  const [before, setBefore] = useState<ReadonlySet<string> | null>(null);
  const devices = useQuery({
    queryKey: DEVICES_KEY,
    queryFn: () => deviceSyncApi.listDevices(),
    enabled: holding,
    refetchInterval: sent ? HOLDER_FINISH_POLL_MS : false,
  });
  const list = devices.data;
  const joined = sent && before ? newlyJoinedDevice(before, list) : null;

  // What was on the list before the details were sent: the answer of a fresh read at the start of
  // the pairing (never the cached list, which may predate a device that joined since). If that
  // answer comes after `sent`, it is the baseline then. A new pairing starts over.
  if (!holding && before !== null) setBefore(null);
  const refetchDevices = devices.refetch;
  useEffect(() => {
    if (!holding) return;
    let cancelled = false;
    void refetchDevices().then((result) => {
      if (cancelled || result.isError || !result.data) return;
      const ids = result.data.map((device) => device.id);
      setBefore((current) => current ?? new Set(ids));
    });
    return () => {
      cancelled = true;
    };
  }, [holding, refetchDevices]);

  // The `expiresAt` of the pairing whose two minutes ran out (it identifies the pairing)
  const pairingKey = view?.expiresAt;
  const [lateFor, setLateFor] = useState<number | null>(null);
  const waiting = sent && joined === null;
  useEffect(() => {
    if (!waiting || pairingKey === undefined) return;
    const timer = setTimeout(() => setLateFor(pairingKey), HOLDER_FINISH_LATE_MS);
    return () => clearTimeout(timer);
  }, [waiting, pairingKey]);
  const late = waiting && lateFor === pairingKey;

  return { joined: joined?.name ?? null, late };
}
