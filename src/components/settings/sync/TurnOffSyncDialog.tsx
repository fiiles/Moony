import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';

/**
 * Turning sync off on this device (password needed): local data stays, the other devices keep
 * syncing. Changes not sent yet are counted, since they will not be sent.
 */
export function TurnOffSyncDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { turnOff } = useDeviceSyncMutations();
  const pending = useDeviceSyncStatus().data?.pendingChanges ?? 0;
  // The password is the mutation's variable: it leaves the cache with the dialog.
  const resetTurnOff = turnOff.reset;
  useEffect(() => resetTurnOff, [resetTurnOff]);
  return (
    <SyncPasswordDialog
      title={t('turnOff.title')}
      description={t('turnOff.description')}
      note={pending > 0 ? t('turnOff.pendingChanges', { count: pending }) : undefined}
      confirmLabel={t('turnOff.confirm')}
      destructive
      onSubmit={async (password) => {
        await turnOff.mutateAsync(password);
        toast(t('turnOff.success'));
      }}
      onClose={onClose}
    />
  );
}
