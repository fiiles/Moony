import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';
import { useLeftoverSyncProfile } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';

/**
 * The one sync row of a build without sync: shown only when this profile has sync on (a
 * development build enabled it), whose changes this build still records (audit M12). It turns
 * sync off behind the device password; with the flag on, Settings → Sync does all of this.
 */
export function LeftoverSyncCard() {
  const { t } = useTranslation('deviceSync');
  const leftover = useLeftoverSyncProfile();
  const { turnOff } = useDeviceSyncMutations();
  const [open, setOpen] = useState(false);
  // The password is the mutation's variable: it leaves the cache with the dialog.
  const resetTurnOff = turnOff.reset;
  useEffect(() => resetTurnOff, [resetTurnOff]);

  if (!leftover) return null;
  return (
    <SettingsCard title={t('title')}>
      <SettingsRow label={t('leftover.text')}>
        <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
          {t('leftover.button')}
        </Button>
      </SettingsRow>
      {open && (
        <SyncPasswordDialog
          title={t('turnOff.title')}
          description={`${t('turnOff.description')} ${t('turnOff.noCutOff')}`}
          confirmLabel={t('turnOff.confirm')}
          destructive
          onSubmit={async (password) => {
            await turnOff.mutateAsync(password);
            toast(t('turnOff.success'));
          }}
          onClose={() => setOpen(false)}
        />
      )}
    </SettingsCard>
  );
}
