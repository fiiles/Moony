import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';
import { ResyncDeviceDialog } from '@/components/settings/sync/SyncRepairDialogs';
import { TurnOffSyncDialog } from '@/components/settings/sync/TurnOffSyncDialog';
import { Button } from '@/components/ui/button';

type Open = 'turnOff' | 'delete' | 'resync' | null;

/** Settings → Sync, sync on: turn it off here, delete what the sync server keeps, re-sync this device. */
export function SyncDangerCard() {
  const { t } = useTranslation('deviceSync');
  const { deleteSyncedData } = useDeviceSyncMutations();
  const [open, setOpen] = useState<Open>(null);
  const close = () => setOpen(null);

  return (
    <>
      <SettingsCard danger title={t('danger.title')} description={t('danger.description')}>
        <SettingsRow label={t('turnOff.button')} hint={t('turnOff.hint')}>
          <Button type="button" variant="outline" onClick={() => setOpen('turnOff')}>
            {t('turnOff.button')}
          </Button>
        </SettingsRow>
        <SettingsRow label={t('delete.button')} hint={t('delete.hint')}>
          <Button type="button" variant="danger" onClick={() => setOpen('delete')}>
            {t('delete.button')}
          </Button>
        </SettingsRow>
        <SettingsRow label={t('resync.button')} hint={t('resync.hint')}>
          <Button type="button" variant="ghost" onClick={() => setOpen('resync')}>
            {t('resync.button')}
          </Button>
        </SettingsRow>
      </SettingsCard>
      {open === 'turnOff' && <TurnOffSyncDialog onClose={close} />}
      {open === 'resync' && <ResyncDeviceDialog onClose={close} />}
      {open === 'delete' && (
        <SyncPasswordDialog
          title={t('delete.title')}
          description={t('delete.description')}
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
