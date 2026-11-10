import { useState } from 'react';
import { SyncIntroCard } from '@/components/settings/sync/SyncIntroCard';
import { SyncDangerCard } from '@/components/settings/sync/SyncDangerCard';
import { SyncDevicesCard } from '@/components/settings/sync/SyncDevicesCard';
import { SyncStatusCard } from '@/components/settings/sync/SyncStatusCard';
import { SyncSetupDialog } from '@/components/settings/sync/SyncSetupDialog';
import { JoinSyncDialog } from '@/components/settings/sync/JoinSyncDialog';
import { RecoveryJoinDialog } from '@/components/settings/sync/RecoveryJoinDialog';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';

type Open = 'setup' | 'pair' | 'kit' | null;

/**
 * Settings → Sync. While sync is off or half set up: the intro card and its dialogs (setup, join by
 * pairing, join with a Recovery Kit; their open state lives here). Sync on: the status card (with
 * the problem alert), the devices card and the danger card.
 */
export function SyncSection() {
  const { data: status } = useDeviceSyncStatus();
  const [open, setOpen] = useState<Open>(null);

  if (status && (status.phase === 'off' || status.phase === 'settingUp')) {
    const close = () => setOpen(null);
    return (
      <>
        <SyncIntroCard
          resuming={status.phase === 'settingUp'}
          onStart={() => setOpen('setup')}
          onJoinByPairing={() => setOpen('pair')}
          onJoinWithKit={() => setOpen('kit')}
        />
        {open === 'setup' && <SyncSetupDialog onClose={close} />}
        {open === 'pair' && <JoinSyncDialog onClose={close} />}
        {open === 'kit' && <RecoveryJoinDialog onClose={close} />}
      </>
    );
  }

  if (!status) return <></>;
  return (
    <>
      <SyncStatusCard status={status} />
      <SyncDevicesCard />
      <SyncDangerCard />
    </>
  );
}
