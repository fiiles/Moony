import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { RefreshCw, Settings } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useAttachmentsStatusText } from '@/hooks/use-attachments-status-text';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { hostOf } from '@/utils/sync-setup';
import { syncAge, type SyncStateView } from '@/utils/sync-state';
import type { SyncStatus } from '@shared/schema';

/**
 * The Synchronizace section of the top-bar "Stav dat" popover: where sync stands, what is waiting,
 * the server, and its own action under it (Synchronizovat, or Otevřít nastavení once it needs the
 * person). Rendered only while sync is on.
 */
export function SyncStatusSection({
  status,
  indicator,
  now,
  onNavigate,
}: {
  status: SyncStatus;
  indicator: SyncStateView;
  now: number;
  onNavigate: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { syncNow } = useDeviceSyncMutations();
  const attachments = useAttachmentsStatusText(status.attachments);

  const label = t(indicator.labelKey, { count: indicator.days });
  const age = indicator.ageFrom === null ? null : syncAge(indicator.ageFrom, now);
  const stale = indicator.tone === 'stale';
  const running = syncNow.isPending || status.phase === 'syncing';

  const handleSyncNow = async () => {
    if (running) return;
    try {
      await syncNow.mutateAsync();
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  return (
    <section>
      <div className="text-eyebrow uppercase text-ink-4">{t('popover.title')}</div>
      <p className={stale ? 'mt-1 text-caption text-loss' : 'mt-1 text-caption text-ink-3'}>
        {age ? `${label} · ${t(`age.${age.key}`, { count: age.count })}` : label}
      </p>
      {stale && status.problem && (
        <p className="mt-1 text-caption text-ink-3">{t(`status.problem.${status.problem}`)}</p>
      )}
      {attachments.problem && <p className="mt-1 text-caption text-ink-3">{attachments.problem}</p>}
      {attachments.failedUpload && (
        <p className="mt-1 text-caption text-ink-3">{attachments.failedUpload}</p>
      )}

      <div className="mt-1 divide-y divide-line-soft text-table">
        <div className="flex items-center justify-between gap-4 py-2">
          <span className="text-ink-3">{t('popover.pending')}</span>
          <span className="font-600 text-ink">{status.pendingChanges}</span>
        </div>
        <div className="flex items-center justify-between gap-4 py-2">
          <span className="text-ink-3">{t('attachments.title')}</span>
          <span className="text-right font-600 text-ink">{attachments.value}</span>
        </div>
        {status.serverAddress && (
          <div className="flex items-center justify-between gap-4 py-2">
            <span className="text-ink-3">{t('popover.server')}</span>
            <span className="font-600 text-ink">
              {hostOf(status.serverAddress)}
              {status.serverInsecure && ` · ${t('server.insecure')}`}
            </span>
          </div>
        )}
      </div>

      <div className="mt-2.5 flex items-center justify-between gap-4">
        {stale ? (
          <Button asChild variant="outline" size="sm">
            <Link href="/settings/sync" onClick={onNavigate}>
              <Settings />
              {t('popover.openSettings')}
            </Link>
          </Button>
        ) : (
          <>
            <Button variant="outline" size="sm" onClick={handleSyncNow} loading={running}>
              <RefreshCw />
              {running ? t('indicator.syncing') : t('status.syncNow')}
            </Button>
            <Link
              href="/settings/sync"
              onClick={onNavigate}
              className="rounded-r1 text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus"
            >
              {t('popover.settings')} →
            </Link>
          </>
        )}
      </div>
    </section>
  );
}
