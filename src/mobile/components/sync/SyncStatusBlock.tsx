import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AlertTriangle, Clock, Download, RefreshCw } from 'lucide-react';
import type { HeldGapView, SyncStatus } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useAttachmentsStatusText } from '@/hooks/use-attachments-status-text';
import { useHeldGaps } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { cn } from '@/lib/utils';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { sentences } from '@/utils/sentences';
import {
  aheadGap,
  aheadText,
  firstSkippableAt,
  heldDays,
  heldFor,
  heldGapDevice,
  offersResync,
  problemAction,
  skippableGap,
  type SyncProblemAction,
} from '@/utils/device-sync-problem';
import { secondsToDate } from '@/utils/sync-status';
import { hostOf } from '@/utils/sync-setup';
import { offerDownloadEverything, onOtherDevices } from '@/utils/sync-attachments';
import {
  isRecentlyOutOfReach,
  serverOutOfReach,
  syncProblemState,
  syncState,
} from '@/utils/sync-state';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { SyncNotices } from '@/mobile/components/sync/SyncNotices';

/** The card the status lines sit in (`.m-card`). */
const CARD = 'mb-3.5 rounded-r4 border border-line bg-paper px-3 shadow-e1';

/** The 48 pt button under the card. */
const BUTTON =
  'h-auto min-h-12 whitespace-normal py-2 text-center w-full rounded-[10px] text-m-row';

interface SyncStatusBlockProps {
  status: SyncStatus;
  /** A repair that could not send every change: re-syncing is offered next to it. */
  offerResync: boolean;
  onRepair: () => void;
  onResync: () => void;
  onTurnOff: () => void;
  /** "Připojit znovu" (the rejoin and waiting cards): the password question opens. */
  onRejoin: () => void;
  /** A change from another device missing for 14 days or more: its skip confirmation opens. */
  onSkipMissing: (gap: HeldGapView) => void;
}

/**
 * Settings → Sync, sync on: the problem that paused or stopped sync with the action that answers it,
 * where sync stands (the phase, when it last finished, what waits to be sent) and "Sync now". A
 * lapsed subscription is renewed elsewhere: the phone never opens a checkout (spec §16a), so on
 * Moony's own sync server the alert only says so, and on a self-hosted one it points at the operator.
 */
export function SyncStatusBlock({
  status,
  offerResync,
  onRepair,
  onResync,
  onTurnOff,
  onRejoin,
  onSkipMissing,
}: SyncStatusBlockProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { dateTime, date } = useFormat();
  const { syncNow, downloadEverything } = useDeviceSyncMutations();
  const attachments = useAttachmentsStatusText(status.attachments);
  const elsewhere = onOtherDevices(status.attachments);
  const needsServerId = useId();

  const run = async () => {
    if (running) return;
    try {
      await syncNow.mutateAsync();
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  // The phone's button: this shell only runs on a phone (the desktop fetches everything by itself)
  const download = async () => {
    if (downloadEverything.isPending || outOfReach) return;
    try {
      await downloadEverything.mutateAsync();
      toast.success(t('attachments.downloadEverythingStarted'));
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  const now = Math.floor(Date.now() / 1000);
  // The server out of reach is a problem from the first failed round: the state reads red and bold,
  // under 7 days with the line that sync resumes on its own; what needs the server waits
  const outOfReach = serverOutOfReach(status);
  const resumesNote = isRecentlyOutOfReach(status, now);
  // The same state the top bar shows, in the phone's row wording
  const indicator = syncState(status, now);
  // A round that takes too long reads "taking longer than usual": the button then tries again
  const slow = indicator?.kind === 'syncingLong';
  const running = !slow && (syncNow.isPending || status.phase === 'syncing');
  const stateLabel = indicator ? t(indicator.rowKey, { count: indicator.days }) : '';
  // A note, never a problem: this device's clock stands more than a day ahead (audit M6)
  const clockAheadUntil = secondsToDate(status.clockAheadUntil);
  const lastSynced =
    status.lastSuccessAt === null
      ? t('status.neverSynced')
      : dateTime(status.lastSuccessAt, { dateStyle: 'medium', timeStyle: 'short' });

  return (
    <>
      {status.problem && (
        <ProblemAlert
          status={status}
          offerResync={offerResync}
          onRepair={onRepair}
          onResync={onResync}
          onTurnOff={onTurnOff}
          onRejoin={onRejoin}
          onSkipMissing={onSkipMissing}
          onTryAgain={() => void run()}
          tryingAgain={syncNow.isPending}
          renderDate={(seconds) => {
            const when = secondsToDate(seconds);
            return when ? date(when, { dateStyle: 'long' }) : '';
          }}
        />
      )}
      <SyncNotices status={status} />
      {clockAheadUntil && (
        <InlineAlert icon={Clock} muted>
          {t('status.clockAhead', { date: date(clockAheadUntil, { dateStyle: 'long' }) })}
        </InlineAlert>
      )}
      <div className={CARD}>
        <KeyValueRow
          label={t('phone.status')}
          value={
            outOfReach ? (
              <span className="font-600 text-loss" data-testid="sync-state-out-of-reach">
                {stateLabel}
              </span>
            ) : (
              stateLabel
            )
          }
          sub={
            indicator?.kind === 'firstUpload'
              ? t('setup.done.body')
              : resumesNote
                ? t('status.unreachableNote')
                : undefined
          }
          wrap
        />
        <KeyValueRow label={t('phone.lastSynced')} value={lastSynced} wrap />
        <KeyValueRow label={t('phone.pending')} value={status.pendingChanges} wrap />
        <KeyValueRow label={t('attachments.title')} value={attachments.value} wrap />
        {status.serverAddress && (
          <KeyValueRow
            label={t('server.label')}
            value={hostOf(status.serverAddress)}
            sub={status.serverInsecure ? t('server.insecure') : undefined}
            wrap
          />
        )}
      </div>
      {attachments.problem && (
        <p className="m-0 mb-3 px-1 text-m-sub text-ink-3">{attachments.problem}</p>
      )}
      {attachments.failedUpload && (
        <p className="m-0 mb-3 px-1 text-m-sub text-ink-3">{attachments.failedUpload}</p>
      )}
      {elsewhere > 0 && (
        <p className="m-0 mb-3 px-1 text-m-sub text-ink-3">
          {t('attachments.onOtherDevices', { count: elsewhere })}
        </p>
      )}
      {offerDownloadEverything(status.attachments) && (
        <>
          <Button
            variant="secondary"
            className={cn(BUTTON, 'mb-3')}
            onClick={() => void download()}
            loading={downloadEverything.isPending}
            aria-disabled={outOfReach || undefined}
            aria-describedby={outOfReach ? needsServerId : undefined}
          >
            <Download />
            {t('attachments.downloadEverything')}
          </Button>
          {outOfReach && (
            <p id={needsServerId} className="m-0 mb-3 px-1 text-m-sub text-ink-3">
              {t('status.needsServer')}
            </p>
          )}
        </>
      )}
      <Button className={BUTTON} onClick={() => void run()} loading={running}>
        <RefreshCw />
        {running
          ? t('indicator.syncing')
          : slow || outOfReach
            ? t('pairing.tryAgain')
            : t('status.syncNow')}
      </Button>
    </>
  );
}

function ProblemAlert({
  status,
  offerResync,
  onRepair,
  onResync,
  onTurnOff,
  onRejoin,
  onSkipMissing,
  onTryAgain,
  tryingAgain,
  renderDate,
}: SyncStatusBlockProps & {
  onTryAgain: () => void;
  tryingAgain: boolean;
  renderDate: (seconds: number) => string;
}) {
  const { t } = useTranslation('deviceSync');
  const { locale } = useFormat();
  const heldGaps = useHeldGaps(status.problem === 'gap' || status.problem === 'clockAhead');
  const problem = status.problem;
  if (!problem) return null;
  const problemState = syncProblemState(status);
  // A key change another device made, not yet confirmed: the ordinary "Synchronizuji…", no alert
  if (problem === 'keyRotated' && !problemState) return null;
  // Out of reach under 7 days: the engine retries; the status row says so in red with the line that
  // sync resumes, so no alert box repeats it
  if (isRecentlyOutOfReach(status, Math.floor(Date.now() / 1000))) return null;

  const now = Math.floor(Date.now() / 1000);
  const held = heldFor(problem, heldGaps.data);
  // A held change younger than 14 days offers nothing: this device keeps syncing meanwhile
  const skippable = skippableGap(held, now);
  const action = problemAction(problem, status.ownServer, skippable);
  const ahead = problem === 'clockAhead' ? aheadGap(held) : null;
  let title: string | undefined;
  let body = ahead ? aheadText(ahead, t, renderDate) : t(`status.problem.${problem}`);
  const skipFrom = firstSkippableAt(held, now);
  if (skipFrom !== null && renderDate(skipFrom)) {
    const key = problem === 'clockAhead' ? 'alert.clockAhead.skipFrom' : 'alert.gap.skipFrom';
    body = sentences(locale, [body, t(key, { date: renderDate(skipFrom) })]);
  }
  let actions: Array<{ label: string; onClick: () => void; disabled?: boolean }> = [];
  const connectAgain = { label: t('problem.rejoin.button'), onClick: onRejoin };
  if (problemState?.kind === 'rejoin') {
    title = t('problem.rejoin.title');
    body = sentences(locale, [
      t('problem.rejoin.body'),
      problemState.by ? t('problem.rejoin.by', { by: problemState.by }) : null,
    ]);
    actions = [connectAgain];
  } else if (problemState?.kind === 'waitingForRemover') {
    title = t('problem.waiting.title', { by: problemState.by });
    body = t('problem.waiting.body', { by: problemState.by });
    actions = [connectAgain];
  }
  // The rejoin and waiting states have their own button: the problem's ordinary action is skipped
  const effectiveAction: SyncProblemAction = problemState ? 'none' : action;
  switch (effectiveAction) {
    case 'renew':
    case 'askOperator':
      title = status.ownServer ? t('alert.lapsed.titleOwnServer') : t('alert.lapsed.title');
      body =
        action === 'askOperator'
          ? t('alert.lapsed.selfHosted')
          : sentences(locale, [
              t('phone.renewUnavailable'),
              secondsToDate(status.deletesAt) !== null
                ? t('alert.lapsed.deletes', { date: renderDate(status.deletesAt ?? 0) })
                : null,
            ]);
      break;
    case 'turnOffOrRetry':
      body = t('alert.vaultGone.text');
      actions = [
        { label: t('alert.vaultGone.turnOff'), onClick: onTurnOff },
        { label: t('alert.vaultGone.retry'), onClick: onTryAgain, disabled: tryingAgain },
      ];
      break;
    case 'repair':
      actions = [
        { label: t('repair.button'), onClick: onRepair },
        ...(offerResync ? [{ label: t('resync.button'), onClick: onResync }] : []),
      ];
      break;
    case 'updateApp':
      body = t('alert.newerSchema');
      break;
    case 'skipMissing':
      if (skippable?.kind === 'ahead') {
        body = aheadText(skippable, t, renderDate);
        actions = [{ label: t('alert.clockAhead.skip'), onClick: () => onSkipMissing(skippable) }];
      } else if (skippable) {
        const device = heldGapDevice(skippable);
        const days = heldDays(skippable, now);
        body =
          skippable.kind === 'parent'
            ? t('alert.gap.waiting', { device, days })
            : t('alert.gap.missing', { device, days, count: skippable.missingChanges });
        actions = [
          {
            label: t('alert.gap.skip', { count: skippable.missingChanges }),
            onClick: () => onSkipMissing(skippable),
          },
        ];
      }
      break;
    case 'none':
      // Re-syncing fixes only these problems (and a repair that could not send everything)
      if (!problemState && offersResync(problem)) {
        actions = [{ label: t('resync.button'), onClick: onResync }];
      }
      break;
    default: {
      // A new SyncProblemAction must be placed above: this fails to compile until it is
      const unhandled: never = effectiveAction;
      return unhandled;
    }
  }

  return (
    <InlineAlert
      icon={AlertTriangle}
      title={title}
      action={actions.length > 0 ? actions : undefined}
    >
      {body}
    </InlineAlert>
  );
}
