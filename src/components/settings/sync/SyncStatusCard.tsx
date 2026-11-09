import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { SyncStatus } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { isSelfHostedPaidUntil } from '@/utils/sync-status';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { ShowRecoveryKitFlow } from '@/components/settings/sync/ShowRecoveryKitFlow';
import { SyncProblemAlert } from '@/components/settings/sync/SyncProblemAlert';
import { Button } from '@/components/ui/button';

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

  const selfHosted = isSelfHostedPaidUntil(status.paidUntil);
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
            {(selfHosted || status.paidUntil !== null) && (
              <SettingsRow
                label={t('status.subscription')}
                hint={
                  selfHosted
                    ? t('status.selfHosted')
                    : t('status.paidUntil', {
                        date: date(new Date((status.paidUntil ?? 0) * 1000), {
                          dateStyle: 'long',
                        }),
                      })
                }
              />
            )}
            <SettingsRow label={t('status.showKit')}>
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
