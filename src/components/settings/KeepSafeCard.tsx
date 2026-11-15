import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { ShowRecoveryKitFlow } from '@/components/settings/sync/ShowRecoveryKitFlow';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { keepSafeItems } from '@/utils/keep-safe';
import { syncIsOn } from '@/utils/safe-copy';

/**
 * Settings → Security → "Co si uschovat" (spec §7.5): the device password, the device recovery key
 * and, while sync is on, the Sync Recovery Kit, each with what it is for. Nothing is rendered
 * without the sync flag. "Zobrazit" opens the Kit flow, which asks for the device password first.
 */
export function KeepSafeCard() {
  const { t } = useTranslation('settings');
  const featureOn = useDeviceSyncFeature();
  const { data: status } = useDeviceSyncStatus();
  const [kitOpen, setKitOpen] = useState(false);

  const items = keepSafeItems(featureOn, syncIsOn(featureOn, status));
  if (items.length === 0) return null;

  return (
    <SettingsCard title={t('keepSafe.title')} description={t('keepSafe.description')}>
      {items.map((item) => (
        <SettingsRow
          key={item}
          label={t(`keepSafe.${item}.title`)}
          hint={t(`keepSafe.${item}.text`)}
        >
          {item === 'kit' && (
            <Button type="button" variant="outline" onClick={() => setKitOpen(true)}>
              {t('keepSafe.kit.show')}
            </Button>
          )}
        </SettingsRow>
      ))}
      {kitOpen && <ShowRecoveryKitFlow onClose={() => setKitOpen(false)} />}
    </SettingsCard>
  );
}
