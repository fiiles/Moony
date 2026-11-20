import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { RefreshCw, Settings } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useAttachmentsStatusText } from '@/hooks/use-attachments-status-text';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { problemTextKey } from '@/utils/device-sync-problem';
import { hostOf } from '@/utils/sync-setup';
import { useFormat } from '@/lib/use-format';
import {
  serverOutOfReach,
  statusPopoverLines,
  syncAge,
  syncProblemState,
  type SyncStateView,
} from '@/utils/sync-state';
import type { SyncStatus } from '@shared/schema';

/**
 * The Synchronizace section of the top-bar "Stav dat" popover: where sync stands, what is waiting,
 * the server, and its own action under it (Synchronizovat, Zkusit znovu while the server is out of
 * reach, or Otevřít nastavení once it needs the person). A problem reads red and bold. Rendered
 * only while sync is on.
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
  const { date } = useFormat();

  const label = t(indicator.labelKey, { count: indicator.days });
  const age = indicator.ageFrom === null ? null : syncAge(indicator.ageFrom, now);
  const stale = indicator.tone === 'stale';
  // The server out of reach is a problem the engine retries by itself: the person can only try
  // again, so the button stays instead of "Otevřít nastavení"
  const outOfReach = serverOutOfReach(status);
  const needsPerson = stale && !outOfReach;
  // A round that takes too long reads "taking longer than usual": the button then tries again
  const slow = indicator.kind === 'syncingLong';
  const running = !slow && (syncNow.isPending || status.phase === 'syncing');
  const problemState = syncProblemState(status);
  const lines = statusPopoverLines(status);

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
      <p
        className={stale ? 'mt-1 text-caption font-650 text-loss' : 'mt-1 text-caption text-ink-3'}
        data-testid="popover-sync-state"
      >
        {age ? `${label} · ${t(`age.${age.key}`, { count: age.count })}` : label}
      </p>
      {stale && status.problem && (
        <p className="mt-1 text-caption text-ink-3">
          {problemState?.kind === 'waitingForRemover'
            ? t('problem.waiting.title', { by: problemState.by })
            : problemState?.kind === 'rejoin'
              ? t('problem.rejoin.title')
              : t(problemTextKey(status.problem, status))}
        </p>
      )}
      {indicator.kind === 'firstUpload' && (
        <p className="mt-1 text-caption text-ink-3">{t('setup.done.body')}</p>
      )}
      {lines.map((line) => (
        <p
          key={line.kind}
          className="mt-1 text-caption text-ink-3"
          data-testid={`popover-line-${line.kind}`}
        >
          {t(line.key, {
            ...line.values,
            ...Object.fromEntries(
              line.dates.map((name) => [
                name,
                date(new Date(Number(line.values[name]) * 1000), { dateStyle: 'long' }),
              ])
            ),
          })}
        </p>
      ))}
      {/* Documents and photos are data like any other: a line only when there is something to say */}
      {attachments.waiting && <p className="mt-1 text-caption text-ink-3">{attachments.waiting}</p>}
      {attachments.failedUpload && (
        <p className="mt-1 text-caption text-ink-3">{attachments.failedUpload}</p>
      )}
      {attachments.problem && (
        <p className="mt-1 text-caption font-650 text-loss">{attachments.problem}</p>
      )}

      <div className="mt-1 divide-y divide-line-soft text-table">
        <div className="flex items-center justify-between gap-4 py-2">
          <span className="text-ink-3">{t('popover.pending')}</span>
          <span className="font-600 text-ink">{status.pendingChanges}</span>
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
        {needsPerson ? (
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
              {running
                ? t('indicator.syncing')
                : slow || outOfReach
                  ? t('pairing.tryAgain')
                  : t('status.syncNow')}
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

/**
 * The Synchronizace section of the popover while sync is off but this device was removed (or left):
 * the one line that says so, until "Rozumím" in Settings, with the way to Settings.
 */
export function SyncRemovedSection({
  status,
  onNavigate,
}: {
  status: SyncStatus;
  onNavigate: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const lines = statusPopoverLines(status);
  if (lines.length === 0) return null;
  return (
    <section>
      <div className="text-eyebrow uppercase text-ink-4">{t('popover.title')}</div>
      {lines.map((line) => (
        <p
          key={line.kind}
          className="mt-1 text-caption text-ink-3"
          data-testid={`popover-line-${line.kind}`}
        >
          {t(line.key, line.values)}
        </p>
      ))}
      <div className="mt-2.5">
        <Button asChild variant="outline" size="sm">
          <Link href="/settings/sync" onClick={onNavigate}>
            <Settings />
            {t('popover.openSettings')}
          </Link>
        </Button>
      </div>
    </section>
  );
}
