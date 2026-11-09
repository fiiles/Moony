import { useMutation, useQueryClient } from '@tanstack/react-query';
import { deviceSyncApi, portfolioApi } from '@/lib/tauri-api';
import { refreshAfterRemoteChanges } from '@/lib/device-sync-refresh';

/**
 * One mutation per `deviceSyncApi` write. Thin on purpose: the setup and Sync screens decide how a
 * failure is shown. Every one re-reads the status; the ones that change local data (finish setup,
 * turn off, delete, repair, re-sync) also run the full rule-7 refresh.
 */
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
    onSuccess: refreshLocalData,
  });
  const repairDevice = useMutation({
    mutationFn: deviceSyncApi.repairDevice,
    onSuccess: refreshLocalData,
  });
  const resyncDevice = useMutation({
    mutationFn: deviceSyncApi.resyncDevice,
    onSuccess: refreshLocalData,
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
    devGrant,
  };
}
