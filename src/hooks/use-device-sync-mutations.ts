import { useEffect } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { deviceSyncApi, portfolioApi } from '@/lib/tauri-api';
import { refreshAfterRemoteChanges } from '@/lib/device-sync-refresh';
import type { ReplaceConsentView } from '@shared/schema';

type KitJoin = Parameters<typeof deviceSyncApi.joinWithRecoveryKit>[0] & {
  consent: ReplaceConsentView;
};

/**
 * One mutation per `deviceSyncApi` write. Thin on purpose: the setup and Sync screens decide how a
 * failure is shown. Every one re-reads the status; the ones that change local data (finish setup,
 * turn off, delete, repair, re-sync) also run the full rule-7 refresh.
 */
/**
 * Key shared by the mutations that replace this device's data (repair, re-sync, joins): the app-wide
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
    await queryClient.invalidateQueries({ queryKey: ['device-sync-backups'] });
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
  // gcTime 0: the kit holds the vault key, so its result leaves the mutation cache with the screen
  // (and the password with it). The device password is required every time (audit M11); until
  // the setup screens ask for it, a call without one reads `validation.syncPasswordNeeded`.
  const prepareRecoveryKit = useMutation({
    mutationFn: (password: string | void) =>
      deviceSyncApi.prepareRecoveryKit(typeof password === 'string' ? password : ''),
    gcTime: 0,
  });
  // gcTime 0 below: a password typed for turning off, delete, repair or re-sync is a mutation
  // variable and must not linger.
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
  // No password: cancelling a setup that has not finished (with sync on it is required).
  const turnOff = useMutation({
    mutationFn: (password: string | void) =>
      deviceSyncApi.turnOff(typeof password === 'string' ? password : undefined),
    gcTime: 0,
    onSuccess: refreshLocalData,
  });
  const deleteSyncedData = useMutation({
    mutationFn: deviceSyncApi.deleteSyncedData,
    gcTime: 0,
    onSuccess: refreshLocalData,
  });
  const repairDevice = useMutation({
    mutationKey: REPLACE_DEVICE_MUTATION_KEY,
    mutationFn: (password: string) => deviceSyncApi.repairDevice(password),
    gcTime: 0,
    onSuccess: refreshLocalData,
  });
  const resyncDevice = useMutation({
    mutationKey: REPLACE_DEVICE_MUTATION_KEY,
    mutationFn: ({ password, consent }: { password: string; consent: ReplaceConsentView }) =>
      deviceSyncApi.resyncDevice(password, consent),
    gcTime: 0,
    onSuccess: refreshLocalData,
  });
  // Joining with a kit replaces this device's data, like repair and re-sync.
  const joinWithRecoveryKit = useMutation({
    mutationKey: REPLACE_DEVICE_MUTATION_KEY,
    mutationFn: ({ consent, ...args }: KitJoin) => deviceSyncApi.joinWithRecoveryKit(args, consent),
    gcTime: 0,
    onSuccess: refreshLocalData,
  });
  // The password stays in the mutation state until the join succeeds; a dialog that closes
  // earlier (error, dismissed) calls `joinWithRecoveryKit.reset()`.
  useEffect(() => {
    if (joinWithRecoveryKit.isSuccess) joinWithRecoveryKit.reset();
  }, [joinWithRecoveryKit, joinWithRecoveryKit.isSuccess]);
  const deleteBackup = useMutation({
    mutationFn: deviceSyncApi.deleteBackup,
    gcTime: 0,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['device-sync-backups'] }),
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
    deleteBackup,
    renameDevice,
    devGrant,
  };
}
