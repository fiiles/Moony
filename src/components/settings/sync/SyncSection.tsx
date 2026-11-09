import { useTranslation } from 'react-i18next';
import { SettingsCard } from '@/components/settings/SettingsCard';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';

/**
 * Settings → Sync. For now the phase and, when there is one, the problem; the setup dialog and
 * the screens for sync on replace this body by phase.
 */
export function SyncSection() {
  const { t } = useTranslation('deviceSync');
  const { data: status } = useDeviceSyncStatus();

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
