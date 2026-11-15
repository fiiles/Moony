import { useState } from 'react';
import { SyncIntroCard } from '@/components/settings/sync/SyncIntroCard';
import { SyncDangerCard } from '@/components/settings/sync/SyncDangerCard';
import { SyncDevicesCard } from '@/components/settings/sync/SyncDevicesCard';
import { SyncStatusCard } from '@/components/settings/sync/SyncStatusCard';
import { SyncSetupDialog } from '@/components/settings/sync/SyncSetupDialog';
import { JoinDeviceDialog } from '@/components/settings/sync/join/JoinDeviceDialog';
import { WhatSyncsLink } from '@/components/settings/sync/WhatSyncsLink';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';

type Open = 'setup' | 'join' | null;

/**
 * Settings → Sync. While sync is off or half set up: the intro card and its dialogs (setup, and the
 * join with a phone or a Recovery Kit; their open state lives here). Sync on: the status card (with
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
          onJoin={() => setOpen('join')}
        />
        <WhatSyncsLink />
        {open === 'setup' && <SyncSetupDialog onClose={close} />}
        {open === 'join' && <JoinDeviceDialog onClose={close} />}
      </>
    );
  }

  if (!status) return <></>;
  return (
    <>
      <SyncStatusCard status={status} />
      <SyncDevicesCard />
      <SyncDangerCard />
      <WhatSyncsLink />
    </>
  );
}
