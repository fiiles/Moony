import { useMutation, useQueryClient } from '@tanstack/react-query';
import { deviceSyncApi, portfolioApi } from '@/lib/tauri-api';
import { refreshAfterRemoteChanges } from '@/lib/device-sync-refresh';

/**
 * One mutation per `deviceSyncApi` write. Thin on purpose: the setup and Sync screens decide how a
 * failure is shown. Every one re-reads the status; the ones that change local data (finish setup,
 * turn off, delete, repair, re-sync) also run the full rule-7 refresh.
 */
/**
 * Key shared by the mutations that replace this device's data (repair, re-sync): the app-wide
 * busy modal counts them with `useIsMutating`, whichever component started them.
 */
export const REPLACE_DEVICE_MUTATION_KEY = ['device-sync-replace-data'] as const;

export function useDeviceSyncMutations() {
  const queryClient = useQueryClient();

  const refreshSyncState = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['device-sync-status'] }),
      queryClient.invalidateQueries({ queryKey: ['device-sync-account'] }),
    ]);
  };
  const refreshLocalData = async () => {
    await refreshSyncState();
    await refreshAfterRemoteChanges(queryClient, portfolioApi.recordSnapshot);
  };

  const createAccount = useMutation({
    mutationFn: (relayUrl?: string) => deviceSyncApi.createAccount(relayUrl),
    onSuccess: refreshSyncState,
  });
  const startCheckout = useMutation({
    mutationFn: deviceSyncApi.startCheckout,
    onSuccess: refreshSyncState,
  });
  // gcTime 0: the kit holds the vault key, so its result leaves the mutation cache with the screen.
  const prepareRecoveryKit = useMutation({
    mutationFn: deviceSyncApi.prepareRecoveryKit,
    gcTime: 0,
  });
  // gcTime 0 below: a password typed for delete / re-sync is a mutation variable and must not linger.
  const copyRecoveryCode = useMutation({ mutationFn: deviceSyncApi.copyRecoveryCode });
  const finishSetup = useMutation({
    mutationFn: ({ deviceName, kitSaved }: { deviceName: string; kitSaved: boolean }) =>
      deviceSyncApi.finishSetup(deviceName, kitSaved),
    onSuccess: refreshLocalData,
  });
  const syncNow = useMutation({
    mutationFn: deviceSyncApi.syncNow,
    onSuccess: refreshSyncState,
  });
  const showRecoveryKit = useMutation({
    mutationFn: deviceSyncApi.showRecoveryKit,
    gcTime: 0,
  });
  const turnOff = useMutation({
    mutationFn: deviceSyncApi.turnOff,
    onSuccess: refreshLocalData,
  });
  const deleteSyncedData = useMutation({
    mutationFn: deviceSyncApi.deleteSyncedData,
    gcTime: 0,
    onSuccess: refreshLocalData,
  });
  const repairDevice = useMutation({
    mutationKey: REPLACE_DEVICE_MUTATION_KEY,
    mutationFn: deviceSyncApi.repairDevice,
    gcTime: 0,
    onSuccess: refreshLocalData,
  });
  const resyncDevice = useMutation({
    mutationKey: REPLACE_DEVICE_MUTATION_KEY,
    mutationFn: deviceSyncApi.resyncDevice,
    gcTime: 0,
    onSuccess: refreshLocalData,
  });
  // Joining with a kit replaces this device's data, like repair and re-sync.
  const joinWithRecoveryKit = useMutation({
    mutationKey: REPLACE_DEVICE_MUTATION_KEY,
    mutationFn: deviceSyncApi.joinWithRecoveryKit,
    gcTime: 0,
    onSuccess: refreshLocalData,
  });
  const renameDevice = useMutation({
    mutationFn: deviceSyncApi.renameDevice,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['device-sync-devices'] }),
  });
  const devGrant = useMutation({
    mutationFn: deviceSyncApi.devGrant,
    onSuccess: refreshSyncState,
  });

  return {
    createAccount,
    startCheckout,
    prepareRecoveryKit,
    copyRecoveryCode,
    finishSetup,
    syncNow,
    showRecoveryKit,
    turnOff,
    deleteSyncedData,
    repairDevice,
    resyncDevice,
    joinWithRecoveryKit,
    renameDevice,
    devGrant,
  };
}
