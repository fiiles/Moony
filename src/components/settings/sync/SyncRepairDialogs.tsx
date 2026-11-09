import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { backupFileName } from '@/utils/sync-status';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';

/** The key the backend sends when a repair could not send every change and replaced nothing. */
export const DRAIN_INCOMPLETE_KEY = 'validation.syncRepairDrainIncomplete';

/** Repair a device whose change tracking is damaged (after a backup, replaces its data). */
export function RepairDeviceDialog({
  onClose,
  onDrainIncomplete,
}: {
  onClose: () => void;
  /** The repair stopped because not every change could be sent: re-syncing is the way on. */
  onDrainIncomplete: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { repairDevice } = useDeviceSyncMutations();

  const run = async () => {
    onClose();
    try {
      const path = await repairDevice.mutateAsync();
      toast(t('repair.success', { file: backupFileName(path) }));
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
      if (error instanceof ApiError && error.key === DRAIN_INCOMPLETE_KEY) onDrainIncomplete();
    }
  };

  return (
    <AlertDialog open onOpenChange={(open) => !open && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('repair.title')}</AlertDialogTitle>
          <AlertDialogDescription>{t('repair.description')}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{tc('buttons.cancel')}</AlertDialogCancel>
          <Button type="button" variant="destructive" onClick={run}>
            {t('repair.confirm')}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Re-sync this device: backup, then its data is replaced by the synced data (password needed). */
export function ResyncDeviceDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { resyncDevice } = useDeviceSyncMutations();
  return (
    <SyncPasswordDialog
      title={t('resync.title')}
      description={t('resync.description')}
      confirmLabel={t('resync.confirm')}
      destructive
      onSubmit={async (password) => {
        const path = await resyncDevice.mutateAsync(password);
        toast(t('resync.success', { file: backupFileName(path) }));
      }}
      onClose={onClose}
    />
  );
}
