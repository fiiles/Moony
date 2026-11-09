import { useState } from 'react';
import { SyncIntroCard } from '@/components/settings/sync/SyncIntroCard';
import { SyncDangerCard } from '@/components/settings/sync/SyncDangerCard';
import { SyncStatusCard } from '@/components/settings/sync/SyncStatusCard';
import { SyncSetupDialog } from '@/components/settings/sync/SyncSetupDialog';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';

/**
 * Settings → Sync. While sync is off or half set up: the intro card and the setup dialog (its open
 * state lives here). Sync on: the status card (with the problem alert) and the danger card.
 */
export function SyncSection() {
  const { data: status } = useDeviceSyncStatus();
  const [setupOpen, setSetupOpen] = useState(false);

  if (status && (status.phase === 'off' || status.phase === 'settingUp')) {
    return (
      <>
        <SyncIntroCard resuming={status.phase === 'settingUp'} onStart={() => setSetupOpen(true)} />
        {setupOpen && <SyncSetupDialog onClose={() => setSetupOpen(false)} />}
      </>
    );
  }

  if (!status) return <></>;
  return (
    <>
      <SyncStatusCard status={status} />
      <SyncDangerCard />
    </>
  );
}
