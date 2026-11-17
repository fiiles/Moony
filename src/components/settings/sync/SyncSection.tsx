import { useState } from 'react';
import { SyncIntroCard } from '@/components/settings/sync/SyncIntroCard';
import { SyncDangerCard } from '@/components/settings/sync/SyncDangerCard';
import { SyncDevicesCard } from '@/components/settings/sync/SyncDevicesCard';
import { SyncStatusCard } from '@/components/settings/sync/SyncStatusCard';
import { SyncSetupDialog } from '@/components/settings/sync/SyncSetupDialog';
import { AddDeviceDialog } from '@/components/settings/sync/AddDeviceDialog';
import { JoinDeviceDialog } from '@/components/settings/sync/join/JoinDeviceDialog';
import { RemovedNoticeCard } from '@/components/settings/sync/RemovedNotice';
import { WhatSyncsLink } from '@/components/settings/sync/WhatSyncsLink';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';

type Open = 'setup' | 'join' | 'add' | null;

/**
 * Settings → Sync. While sync is off or half set up: the intro card and its dialogs (setup, and the
 * join with a phone or a Recovery Kit; their open state lives here). Sync on: the status card (with
 * the problem alert), the devices card and the danger card. The dialogs sit after the cards in one
 * place, so a setup that turns sync on keeps its "Synchronizace je zapnutá" screen while the cards
 * change under it; that screen can open "Přidat druhé zařízení".
 */
export function SyncSection() {
  const { data: status } = useDeviceSyncStatus();
  const [open, setOpen] = useState<Open>(null);
  const close = () => setOpen(null);

  let cards = <></>;
  if (status && (status.phase === 'off' || status.phase === 'settingUp')) {
    cards = (
      <>
        {status.removedNotice && (
          <RemovedNoticeCard notice={status.removedNotice} onRejoin={() => setOpen('join')} />
        )}
        <SyncIntroCard
          resuming={status.phase === 'settingUp'}
          onStart={() => setOpen('setup')}
          onJoin={() => setOpen('join')}
        />
        <WhatSyncsLink />
      </>
    );
  } else if (status) {
    cards = (
      <>
        <SyncStatusCard status={status} onRejoin={() => setOpen('join')} />
        <SyncDevicesCard />
        <SyncDangerCard />
        <WhatSyncsLink />
      </>
    );
  }

  return (
    <>
      {cards}
      {open === 'setup' && <SyncSetupDialog onClose={close} onAddDevice={() => setOpen('add')} />}
      {open === 'join' && <JoinDeviceDialog onClose={close} />}
      {open === 'add' && <AddDeviceDialog onClose={close} />}
    </>
  );
}
