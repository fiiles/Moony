import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { StatusText } from '@/components/shell/DataStatus';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { syncIndicator } from '@/utils/device-sync-indicator';

/** Top-bar sync status (`.status`): shown only while sync is on; links to Settings → Sync. */
export function SyncStatusIndicator() {
  const { t } = useTranslation('deviceSync');
  const feature = useDeviceSyncFeature();
  const { data: status } = useDeviceSyncStatus();
  const indicator = feature ? syncIndicator(status) : null;
  if (!indicator) return null;

  return (
    <Link
      href="/settings/sync"
      className="rounded-r1 focus-visible:outline-none focus-visible:shadow-focus"
    >
      <StatusText tone={indicator.tone}>{t(indicator.labelKey)}</StatusText>
    </Link>
  );
}
