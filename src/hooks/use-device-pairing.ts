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
 */
export function useDevicePairing() {
  const queryClient = useQueryClient();
  const enabled = useDeviceSyncFeature();

  const { data: view } = useQuery<PairingView>({
    queryKey: PAIRING_KEY,
    queryFn: () => deviceSyncApi.pairingPoll(),
    // Only once a start or scan put a pairing there; the poll never starts one.
    enabled: enabled && queryClient.getQueryData(PAIRING_KEY) !== undefined,
    refetchInterval: (query) => pairingPollInterval(query.state.data),
    retry: false,
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
  const confirm = useMutation({
    mutationFn: () => deviceSyncApi.pairingConfirm(),
    onSuccess: setView,
  });
  const join = useMutation({
    mutationKey: REPLACE_DEVICE_MUTATION_KEY,
    mutationFn: () => deviceSyncApi.pairingJoin(),
    gcTime: 0,
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['device-sync-status'] }),
        queryClient.invalidateQueries({ queryKey: ['device-sync-account'] }),
      ]);
      await refreshAfterRemoteChanges(queryClient, portfolioApi.recordSnapshot);
    },
  });

  const cancel = async () => {
    try {
      await deviceSyncApi.pairingCancel();
    } finally {
      queryClient.removeQueries({ queryKey: PAIRING_KEY });
    }
  };

  return { view, start, scan, confirm, cancel, join };
}
