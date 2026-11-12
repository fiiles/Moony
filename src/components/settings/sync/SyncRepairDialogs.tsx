import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { consentAsItStands, useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { backupFileName } from '@/utils/sync-status';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';

/** The key the backend sends when a repair could not send every change and replaced nothing. */
export const DRAIN_INCOMPLETE_KEY = 'validation.syncRepairDrainIncomplete';

/**
 * Repair a device whose change tracking is damaged (password needed; after a backup, replaces its
 * data). The dialog stays while the repair runs; a failure shows under the field.
 */
export function RepairDeviceDialog({
  onClose,
  onDrainIncomplete,
}: {
  onClose: () => void;
  /** The repair stopped because not every change could be sent: re-syncing is the way on. */
  onDrainIncomplete: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { repairDevice } = useDeviceSyncMutations();
  // The password is the mutation's variable: it leaves the cache with the dialog.
  const resetRepair = repairDevice.reset;
  useEffect(() => resetRepair, [resetRepair]);
  return (
    <SyncPasswordDialog
      title={t('repair.title')}
      description={t('repair.description')}
      confirmLabel={t('repair.confirm')}
      destructive
      onSubmit={async (password) => {
        try {
          const outcome = await repairDevice.mutateAsync(password);
          toast(t('repair.success', { file: backupFileName(outcome.backupPath) }));
        } catch (error) {
          if (error instanceof ApiError && error.key === DRAIN_INCOMPLETE_KEY) onDrainIncomplete();
          throw error;
        }
      }}
      onClose={onClose}
    />
  );
}

/** Re-sync this device: backup, then its data is replaced by the synced data (password needed). */
export function ResyncDeviceDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { resyncDevice } = useDeviceSyncMutations();
  const pending = useDeviceSyncStatus().data?.pendingChanges ?? 0;
  // The password is the mutation's variable: it leaves the cache with the dialog.
  const resetResync = resyncDevice.reset;
  useEffect(() => resetResync, [resetResync]);
  return (
    <SyncPasswordDialog
      title={t('resync.title')}
      description={t('resync.description')}
      note={pending > 0 ? t('resync.pendingChanges', { count: pending }) : undefined}
      confirmLabel={t('resync.confirm')}
      destructive
      onSubmit={async (password) => {
        const outcome = await resyncDevice.mutateAsync({
          password,
          consent: await consentAsItStands(),
        });
        toast(t('resync.success', { file: backupFileName(outcome.backupPath) }));
      }}
      onClose={onClose}
    />
  );
}
