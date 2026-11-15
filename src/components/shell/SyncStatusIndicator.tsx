import { useState } from 'react';
import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { RefreshCw } from 'lucide-react';
import { StatusText } from '@/components/shell/DataStatus';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useAttachmentsStatusText } from '@/hooks/use-attachments-status-text';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { hostOf } from '@/utils/sync-setup';
import { syncAge, syncState } from '@/utils/sync-state';

/**
 * Top-bar sync status (`.status`), shown only while sync is on. Clicking opens the detail, laid out
 * like `DataStatus`'s: where sync stands, what is waiting, the server, and the everyday actions.
 */
export function SyncStatusIndicator() {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const feature = useDeviceSyncFeature();
  const { data: status } = useDeviceSyncStatus();
  const { syncNow } = useDeviceSyncMutations();
  const [open, setOpen] = useState(false);
  const now = Math.floor(Date.now() / 1000);
  const indicator = feature ? syncState(status, now) : null;
  const attachments = useAttachmentsStatusText(status?.attachments);
  if (!indicator || !status) return null;

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

  const linkClass =
    'rounded-r1 text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus';

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="rounded-r1 px-1 py-0.5 focus-visible:outline-none focus-visible:shadow-focus"
          aria-label={t('popover.title')}
        >
          <StatusText tone={indicator.tone}>{label}</StatusText>
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-80" align="end">
        <div className="space-y-4">
          <div>
            <h4 className="text-h3 text-ink">{t('popover.title')}</h4>
            {stale && status.problem && (
              <p className="mt-1 text-caption text-ink-3">
                {t(`status.problem.${status.problem}`)}
              </p>
            )}
            {attachments.problem && (
              <p className="mt-1 text-caption text-ink-3">{attachments.problem}</p>
            )}
            <p className="mt-1 text-caption text-ink-3">
              {age ? `${label} · ${t(`age.${age.key}`, { count: age.count })}` : label}
            </p>
          </div>

          <div className="divide-y divide-line-soft text-table">
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

          {stale ? (
            <Button asChild variant="outline" size="sm" className="w-full">
              <Link href="/settings/sync" onClick={() => setOpen(false)}>
                {t('popover.openSettings')}
              </Link>
            </Button>
          ) : (
            <div className="flex items-center justify-between gap-4">
              <Button variant="outline" size="sm" onClick={handleSyncNow} loading={running}>
                <RefreshCw />
                {running ? t('indicator.syncing') : t('status.syncNow')}
              </Button>
              <Link href="/settings/sync" onClick={() => setOpen(false)} className={linkClass}>
                {t('popover.settings')} →
              </Link>
            </div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
