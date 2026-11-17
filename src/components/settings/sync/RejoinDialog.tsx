import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';

/**
 * "Připojit znovu" (spec §6.4): the device password, then sync is turned off on this device (its
 * data stay) and `onTurnedOff` opens the join choice. Joining replaces the device's data after a
 * backup; changes that never left it stay in that backup.
 */
export function RejoinDialog({
  onTurnedOff,
  onClose,
}: {
  onTurnedOff: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { turnOff } = useDeviceSyncMutations();
  // The password is the mutation's variable: it leaves the cache with the dialog.
  const resetTurnOff = turnOff.reset;
  useEffect(() => resetTurnOff, [resetTurnOff]);
  return (
    <SyncPasswordDialog
      title={t('rejoin.title')}
      description={t('rejoin.description')}
      confirmLabel={t('problem.rejoin.button')}
      closeOnSuccess={false}
      onSubmit={async (password) => {
        try {
          await turnOff.mutateAsync(password);
        } catch (error) {
          resetTurnOff();
          throw error;
        }
        onTurnedOff();
      }}
      onClose={onClose}
    />
  );
}
