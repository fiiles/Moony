import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { SyncStatus } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { secondsToDate } from '@/utils/sync-status';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { ShowRecoveryKitFlow } from '@/components/settings/sync/ShowRecoveryKitFlow';
import { SyncProblemAlert } from '@/components/settings/sync/SyncProblemAlert';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Clock } from 'lucide-react';

/** Settings → Sync, sync on: where sync stands, what is waiting, and the two everyday actions. */
export function SyncStatusCard({ status }: { status: SyncStatus }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { dateTime, date } = useFormat();
  const { syncNow } = useDeviceSyncMutations();
  const [kitOpen, setKitOpen] = useState(false);

  const handleSyncNow = async () => {
    try {
      await syncNow.mutateAsync();
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  const paidUntil = secondsToDate(status.paidUntil);
  // A note, never a problem: this device's clock stands more than a day ahead (audit M6)
  const clockAheadUntil = secondsToDate(status.clockAheadUntil);
  const lastSynced =
    status.lastSuccessAt === null
      ? t('status.neverSynced')
      : t('status.lastSynced', {
          date: dateTime(status.lastSuccessAt, { dateStyle: 'medium', timeStyle: 'short' }),
        });

  return (
    <>
      <SettingsCard title={t('title')} description={t('description')}>
        <div className="grid gap-3">
          <SyncProblemAlert status={status} />
          {clockAheadUntil && (
            <Alert>
              <Clock />
              <AlertDescription>
                {t('status.clockAhead', { date: date(clockAheadUntil, { dateStyle: 'long' }) })}
              </AlertDescription>
            </Alert>
          )}
          <div>
            <SettingsRow
              label={t(`status.phase.${status.phase}`)}
              hint={`${lastSynced} · ${t('status.pending', { number: status.pendingChanges })}`}
            >
              <Button
                type="button"
                onClick={handleSyncNow}
                loading={syncNow.isPending || status.phase === 'syncing'}
              >
                {t('status.syncNow')}
              </Button>
            </SettingsRow>
            {!status.ownServer && paidUntil && (
              <SettingsRow
                label={t('status.subscription')}
                hint={t('status.paidUntil', { date: date(paidUntil, { dateStyle: 'long' }) })}
              />
            )}
            <SettingsRow label={t('showKit.title')}>
              <Button type="button" variant="outline" onClick={() => setKitOpen(true)}>
                {t('status.showKit')}
              </Button>
            </SettingsRow>
          </div>
        </div>
      </SettingsCard>
      {kitOpen && <ShowRecoveryKitFlow onClose={() => setKitOpen(false)} />}
    </>
  );
}
