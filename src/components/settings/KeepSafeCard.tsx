import { useTranslation } from 'react-i18next';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { keepSafeItems } from '@/utils/keep-safe';
import { syncIsOn } from '@/utils/safe-copy';

/**
 * Settings → Security → "Co si uschovat" (spec §7.5): the device password, the device recovery key
 * and, while sync is on, the Sync Recovery Kit, each with what it is for. Nothing is rendered
 * without the sync flag. The Kit can no longer be shown again (revision §2.2): its row waits for the
 * Kit row of Settings → Sync (Task 27).
 */
export function KeepSafeCard() {
  const { t } = useTranslation('settings');
  const featureOn = useDeviceSyncFeature();
  const { data: status } = useDeviceSyncStatus();

  const items = keepSafeItems(featureOn, syncIsOn(featureOn, status));
  if (items.length === 0) return null;

  return (
    <SettingsCard title={t('keepSafe.title')} description={t('keepSafe.description')}>
      {items.map((item) => (
        <SettingsRow
          key={item}
          label={t(`keepSafe.${item}.title`)}
          hint={t(`keepSafe.${item}.text`)}
        />
      ))}
    </SettingsCard>
  );
}
