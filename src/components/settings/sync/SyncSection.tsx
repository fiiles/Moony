import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SettingsCard } from '@/components/settings/SettingsCard';
import { SyncIntroCard } from '@/components/settings/sync/SyncIntroCard';
import { SyncSetupDialog } from '@/components/settings/sync/SyncSetupDialog';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';

/**
 * Settings → Sync. While sync is off or half set up: the intro card and the setup dialog (its open
 * state lives here). The screens for sync on replace the rest by phase.
 */
export function SyncSection() {
  const { t } = useTranslation('deviceSync');
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

  return (
    <SettingsCard title={t('title')} description={t('description')}>
      {status && (
        <div className="grid gap-1.5 text-body">
          <p className="m-0 font-600 text-ink">{t(`status.phase.${status.phase}`)}</p>
          {status.problem && (
            <p className="m-0 text-ink-3">{t(`status.problem.${status.problem}`)}</p>
          )}
        </div>
      )}
    </SettingsCard>
  );
}
