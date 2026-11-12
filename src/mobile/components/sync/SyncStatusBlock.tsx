import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AlertTriangle } from 'lucide-react';
import type { HeldGapView, SyncStatus } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useHeldGaps } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import {
  firstSkippableAt,
  heldDays,
  heldGapDevice,
  problemAction,
  skippableGap,
} from '@/utils/device-sync-problem';
import { isSelfHostedPaidUntil } from '@/utils/sync-status';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';

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
  onSkipMissing,
}: SyncStatusBlockProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { dateTime, date } = useFormat();
  const { syncNow } = useDeviceSyncMutations();

  const run = async () => {
    try {
      await syncNow.mutateAsync();
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

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
          onSkipMissing={onSkipMissing}
          onTryAgain={() => void run()}
          tryingAgain={syncNow.isPending}
          renderDate={(seconds) => date(new Date(seconds * 1000), { dateStyle: 'long' })}
        />
      )}
      <div className={CARD}>
        <KeyValueRow label={t('phone.status')} value={t(`status.phase.${status.phase}`)} />
        <KeyValueRow label={t('phone.lastSynced')} value={lastSynced} />
        <KeyValueRow label={t('phone.pending')} value={status.pendingChanges} />
        {isSelfHostedPaidUntil(status.paidUntil) && (
          <KeyValueRow label={t('status.subscription')} value={t('status.selfHosted')} />
        )}
      </div>
      <Button
        className={BUTTON}
        onClick={() => void run()}
        loading={syncNow.isPending || status.phase === 'syncing'}
      >
        {t('status.syncNow')}
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
  const heldGaps = useHeldGaps(status.problem === 'gap');
  const problem = status.problem;
  if (!problem) return null;

  const now = Math.floor(Date.now() / 1000);
  // A gap younger than 14 days offers nothing: this device keeps syncing meanwhile
  const skippable = problem === 'gap' ? skippableGap(heldGaps.data, now) : null;
  const action = problemAction(problem, isSelfHostedPaidUntil(status.paidUntil), skippable);
  let title: string | undefined;
  let body = t(`status.problem.${problem}`);
  const skipFrom = problem === 'gap' ? firstSkippableAt(heldGaps.data, now) : null;
  if (skipFrom !== null) {
    body = `${body} ${t('alert.gap.skipFrom', { date: renderDate(skipFrom) })}`;
  }
  let actions: Array<{ label: string; onClick: () => void; disabled?: boolean }> = [];
  switch (action) {
    case 'renew':
    case 'askOperator':
      title = t('alert.lapsed.title');
      body =
        action === 'askOperator'
          ? t('alert.lapsed.selfHosted')
          : [
              t('phone.renewUnavailable'),
              status.deletesAt !== null
                ? t('alert.lapsed.deletes', { date: renderDate(status.deletesAt) })
                : null,
            ]
              .filter(Boolean)
              .join(' ');
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
    case 'resync':
      actions = [{ label: t('resync.button'), onClick: onResync }];
      break;
    case 'updateApp':
      body = t('alert.newerSchema');
      break;
    case 'skipMissing':
      if (skippable) {
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
