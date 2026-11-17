import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useServerGate } from '@/hooks/use-server-gate';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';
import { TurnOffSyncDialog } from '@/components/settings/sync/TurnOffSyncDialog';
import { Button } from '@/components/ui/button';

type Open = 'turnOff' | 'delete' | null;

/**
 * Settings → Sync, sync on: turn it off here, delete what the sync server keeps. "Synchronizovat toto
 * zařízení znovu" is a repair, not a way out: it sits under the problem it answers (`SyncProblemAlert`).
 */
export function SyncDangerCard() {
  const { t } = useTranslation('deviceSync');
  const { deleteSyncedData } = useDeviceSyncMutations();
  const { data: status } = useDeviceSyncStatus();
  const deleteAfterwards = t(
    status?.ownServer ? 'delete.afterwardsOwnServer' : 'delete.afterwards'
  );
  const resetDelete = deleteSyncedData.reset;
  useEffect(() => resetDelete, [resetDelete]);
  const [open, setOpen] = useState<Open>(null);
  // Deleting what the server keeps needs the server; turning sync off here does not
  const server = useServerGate();
  const close = () => setOpen(null);

  return (
    <>
      <SettingsCard danger title={t('danger.title')} description={t('danger.description')}>
        <SettingsRow label={t('turnOff.button')} hint={t('turnOff.hint')}>
          <Button type="button" variant="outline" onClick={() => setOpen('turnOff')}>
            {t('turnOff.button')}
          </Button>
        </SettingsRow>
        <SettingsRow
          label={t('delete.button')}
          hint={
            <>
              {t('delete.hint')}
              {server.blocked && (
                <span id={server.noteId} className="block" data-testid="server-needed">
                  {t('status.needsServer')}
                </span>
              )}
            </>
          }
        >
          <Button
            type="button"
            variant="danger"
            onClick={() => setOpen('delete')}
            {...server.props}
          >
            {t('delete.button')}
          </Button>
        </SettingsRow>
      </SettingsCard>
      {open === 'turnOff' && <TurnOffSyncDialog onClose={close} />}
      {open === 'delete' && (
        <SyncPasswordDialog
          needsServer
          title={t('delete.title')}
          description={`${t('delete.description')} ${deleteAfterwards}`}
          confirmLabel={t('delete.confirm')}
          destructive
          onSubmit={async (password) => {
            await deleteSyncedData.mutateAsync(password);
            toast(t('delete.success'));
          }}
          onClose={close}
        />
      )}
    </>
  );
}
