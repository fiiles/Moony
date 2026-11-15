import { useEffect, useRef } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { deviceSyncApi, portfolioApi } from '@/lib/tauri-api';
import { refreshAfterRemoteChanges } from '@/lib/device-sync-refresh';
import type { RecoveryKit, ReplaceConsentView } from '@shared/schema';

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
      queryClient.invalidateQueries({ queryKey: ['device-sync-profile-state'] }),
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
  // gcTime 0: the mutation keeps the connection code (its variables) in state, so it leaves the
  // mutation cache with the screen; setup also `reset()`s it when it closes.
  const connectOwnServer = useMutation({
    mutationFn: (connectionCode: string) => deviceSyncApi.connectOwnServer(connectionCode),
    onSuccess: refreshSyncState,
    gcTime: 0,
  });
  const startCheckout = useMutation({
    mutationFn: deviceSyncApi.startCheckout,
    onSuccess: refreshSyncState,
  });
  // gcTime 0: the kit holds the vault key, so its result leaves the mutation cache with the screen.
  // The device password is required every time (audit M11) and TanStack Query keeps a mutation's
  // variables in its state, so both kit mutations take none: the password travels in a ref that
  // `mutationFn` empties (the pattern of `unlockWithPassword` in `use-auth.tsx`). The setup and
  // Show Recovery Kit screens call `prepareKitWithPassword` / `showKitWithPassword`, and `reset()`
  // the mutation when the kit leaves the screen.
  const prepareKitPassword = useRef<string | null>(null);
  const prepareRecoveryKit = useMutation<RecoveryKit, Error, void>({
    mutationFn: async () => {
      const password = prepareKitPassword.current;
      prepareKitPassword.current = null;
      if (password === null) throw new Error('No password to prepare the kit with');
      return deviceSyncApi.prepareRecoveryKit(password);
    },
    gcTime: 0,
  });
  const prepareKitWithPassword = (password: string) => {
    prepareKitPassword.current = password;
    return prepareRecoveryKit.mutateAsync();
  };
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
  // Only the status moves (what is left to fetch); the content itself arrives later, row by row
  const downloadEverything = useMutation({
    mutationFn: deviceSyncApi.downloadEverything,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['device-sync-status'] }),
  });
  const showKitPassword = useRef<string | null>(null);
  const showRecoveryKit = useMutation<RecoveryKit, Error, void>({
    mutationFn: async () => {
      const password = showKitPassword.current;
      showKitPassword.current = null;
      if (password === null) throw new Error('No password to show the kit with');
      return deviceSyncApi.showRecoveryKit(password);
    },
    gcTime: 0,
  });
  const showKitWithPassword = (password: string) => {
    showKitPassword.current = password;
    return showRecoveryKit.mutateAsync();
  };
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
  // The password is a mutation variable: the dialog or sheet that asks for it calls `reset()` when
  // it leaves.
  const changeServerAddress = useMutation({
    mutationFn: ({ password, address }: { password: string; address: string }) =>
      deviceSyncApi.changeServerAddress(password, address),
    gcTime: 0,
    onSuccess: refreshSyncState,
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
  // Skipping a held change moves where this device is in the synced history; the round it asks
  // for brings the changes after it, and their refresh comes with `sync-data-changed`.
  const skipMissingChange = useMutation({
    mutationFn: ({ password, deviceId }: { password: string; deviceId: string }) =>
      deviceSyncApi.skipMissingChange(password, deviceId),
    gcTime: 0,
    onSuccess: async () => {
      await refreshSyncState();
      await queryClient.invalidateQueries({ queryKey: ['device-sync-held-gaps'] });
    },
  });
  const renameDevice = useMutation({
    mutationFn: deviceSyncApi.renameDevice,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['device-sync-devices'] }),
  });

  return {
    createAccount,
    connectOwnServer,
    startCheckout,
    prepareRecoveryKit,
    prepareKitWithPassword,
    copyRecoveryCode,
    finishSetup,
    syncNow,
    downloadEverything,
    showRecoveryKit,
    showKitWithPassword,
    turnOff,
    deleteSyncedData,
    repairDevice,
    resyncDevice,
    changeServerAddress,
    joinWithRecoveryKit,
    deleteBackup,
    skipMissingChange,
    renameDevice,
  };
}
