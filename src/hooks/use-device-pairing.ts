import { useEffect, useRef } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { PairingView } from '@shared/schema';
import { deviceSyncApi, portfolioApi } from '@/lib/tauri-api';
import { refreshAfterRemoteChanges } from '@/lib/device-sync-refresh';
import { pairingPollInterval } from '@/utils/pairing-code';
import { REPLACE_DEVICE_MUTATION_KEY } from './use-device-sync-mutations';
import { useDeviceSyncFeature } from './use-device-sync';

const PAIRING_KEY = ['device-sync-pairing'] as const;

/**
 * The pairing flow on this device. The view lives under `['device-sync-pairing']`: start, scan and
 * confirm write their result there and a query polls the backend while a device is awaited.
 * Joining replaces local data, so it runs the full refresh and shares the busy-cover key.
 *
 * Passwords: `start` and `scan` keep their variables (the device password) in the mutation state,
 * so they are reset as soon as they succeed. A dialog that closes any other way (error, dismissed)
 * must call `cancel()` (resets both and clears the view) or `start.reset()` / `scan.reset()` itself.
 * `error` / `isError` report a failed poll (expired, cancelled by the other device, unreachable);
 * polling stops on an error and once `expiresAt` has passed.
 */
export function useDevicePairing() {
  const queryClient = useQueryClient();
  const enabled = useDeviceSyncFeature();

  const {
    data: view,
    error,
    isError,
  } = useQuery<PairingView>({
    queryKey: PAIRING_KEY,
    queryFn: () => deviceSyncApi.pairingPoll(),
    // Only once a start or scan put a pairing there; the poll never starts one.
    enabled: enabled && queryClient.getQueryData(PAIRING_KEY) !== undefined,
    refetchInterval: (query) =>
      query.state.status === 'error' ||
      (query.state.data !== undefined && query.state.data.expiresAt * 1000 <= Date.now())
        ? false
        : pairingPollInterval(query.state.data),
    retry: false,
    // A finished or abandoned pairing must not keep its QR or code around.
    gcTime: 0,
  });

  const setView = (next: PairingView) => queryClient.setQueryData(PAIRING_KEY, next);

  // gcTime 0 on start and scan: the device password is a mutation variable and must not linger.
  const start = useMutation({
    mutationFn: (opts: { password: string; relayUrl?: string; deviceName?: string }) =>
      deviceSyncApi.pairingStart(opts),
    gcTime: 0,
    onSuccess: setView,
  });
  const scan = useMutation({
    mutationFn: ({ code, ...opts }: { code: string; password: string; deviceName?: string }) =>
      deviceSyncApi.pairingScan(code, opts),
    gcTime: 0,
    onSuccess: setView,
  });
  // The password is no longer needed once the pairing is on its way.
  useEffect(() => {
    if (start.isSuccess) start.reset();
  }, [start, start.isSuccess]);
  useEffect(() => {
    if (scan.isSuccess) scan.reset();
  }, [scan, scan.isSuccess]);
  const confirm = useMutation({
    mutationFn: () => deviceSyncApi.pairingConfirm(),
    onSuccess: setView,
  });
  // A join that succeeded ended the pairing by itself: nothing is left to cancel on unmount.
  const joined = useRef(false);
  const join = useMutation({
    mutationKey: REPLACE_DEVICE_MUTATION_KEY,
    mutationFn: () => deviceSyncApi.pairingJoin(),
    gcTime: 0,
    onSuccess: async () => {
      joined.current = true;
      queryClient.removeQueries({ queryKey: PAIRING_KEY });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['device-sync-status'] }),
        queryClient.invalidateQueries({ queryKey: ['device-sync-account'] }),
      ]);
      await refreshAfterRemoteChanges(queryClient, portfolioApi.recordSnapshot);
    },
  });

  const cancel = async () => {
    start.reset();
    scan.reset();
    try {
      await deviceSyncApi.pairingCancel();
    } finally {
      queryClient.removeQueries({ queryKey: PAIRING_KEY });
    }
  };

  // Whatever closes the dialog (auto-lock, another branch of the page, a route change) gives the
  // pairing up too. Cancelling is idempotent, so the dialogs' own close handlers may cancel first.
  const { reset: resetStart } = start;
  const { reset: resetScan } = scan;
  useEffect(
    () => () => {
      resetStart();
      resetScan();
      if (joined.current) return;
      queryClient.removeQueries({ queryKey: PAIRING_KEY });
      deviceSyncApi.pairingCancel().catch(() => undefined);
    },
    [queryClient, resetStart, resetScan]
  );

  return { view, error, isError, start, scan, confirm, cancel, join };
}
