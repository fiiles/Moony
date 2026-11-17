import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { SyncStatus } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useAttachmentsStatusText } from '@/hooks/use-attachments-status-text';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { secondsToDate } from '@/utils/sync-status';
import { hostOf } from '@/utils/sync-setup';
import { isQuietlyOutOfReach, syncState } from '@/utils/sync-state';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { ChangeServerDialog } from '@/components/settings/sync/ChangeServerDialog';
import { SyncProblemAlert } from '@/components/settings/sync/SyncProblemAlert';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Clock, RefreshCw } from 'lucide-react';

/** Settings → Sync, sync on: where sync stands, what is waiting, and the two everyday actions. */
export function SyncStatusCard({ status }: { status: SyncStatus }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { dateTime, date } = useFormat();
  const { syncNow } = useDeviceSyncMutations();
  const [serverOpen, setServerOpen] = useState(false);

  const running = syncNow.isPending || status.phase === 'syncing';
  const attachments = useAttachmentsStatusText(status.attachments);

  const handleSyncNow = async () => {
    if (running) return;
    try {
      await syncNow.mutateAsync();
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  const now = Math.floor(Date.now() / 1000);
  const unreachable = isQuietlyOutOfReach(status, now);
  // The same label the top bar shows, so the surfaces never contradict each other
  const indicator = syncState(status, now);
  const stateLabel = indicator ? t(indicator.labelKey, { count: indicator.days }) : '';
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
          {unreachable && (
            <p className="m-0 text-caption text-ink-3">{t('status.unreachableNote')}</p>
          )}
          <div>
            <SettingsRow
              label={stateLabel}
              hint={`${lastSynced} · ${t('status.pending', { number: status.pendingChanges })}`}
            >
              <Button type="button" onClick={handleSyncNow} loading={running}>
                <RefreshCw />
                {running ? t('indicator.syncing') : t('status.syncNow')}
              </Button>
            </SettingsRow>
            <SettingsRow
              label={t('attachments.title')}
              hint={
                <>
                  {attachments.value}
                  {attachments.problem && <span className="block">{attachments.problem}</span>}
                  {attachments.failedUpload && (
                    <span className="block">{attachments.failedUpload}</span>
                  )}
                </>
              }
            />
            {!status.ownServer && paidUntil && (
              <SettingsRow
                label={t('status.subscription')}
                hint={t('status.paidUntil', { date: date(paidUntil, { dateStyle: 'long' }) })}
              />
            )}
            {status.serverAddress && (
              <SettingsRow
                label={t('server.label')}
                hint={
                  <>
                    <span className="font-mono">{hostOf(status.serverAddress)}</span>
                    {status.serverInsecure && <span> · {t('server.insecure')}</span>}
                  </>
                }
              >
                <Button type="button" variant="outline" onClick={() => setServerOpen(true)}>
                  {t('server.change')}
                </Button>
              </SettingsRow>
            )}
          </div>
        </div>
      </SettingsCard>
      {serverOpen && <ChangeServerDialog onClose={() => setServerOpen(false)} />}
    </>
  );
}
