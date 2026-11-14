import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AlertTriangle } from 'lucide-react';
import type { HeldGapView, SyncStatus } from '@shared/schema';
import { useHeldGaps } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import {
  aheadGap,
  aheadText,
  firstSkippableAt,
  heldDays,
  heldFor,
  heldGapDevice,
  problemAction,
  skippableGap,
} from '@/utils/device-sync-problem';
import { secondsToDate } from '@/utils/sync-status';
import { syncState } from '@/utils/sync-state';
import { Alert, AlertActions, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { SyncRenewDialog } from '@/components/settings/sync/SyncRenewDialog';
import {
  RepairDeviceDialog,
  ResyncDeviceDialog,
} from '@/components/settings/sync/SyncRepairDialogs';
import { TurnOffSyncDialog } from '@/components/settings/sync/TurnOffSyncDialog';
import { SkipMissingChangeDialog } from '@/components/settings/sync/SkipMissingChangeDialog';

type Open = 'renew' | 'repair' | 'resync' | 'turnOff' | null;

/** The problem that paused or stopped sync, next to its cause, with the action that answers it. */
export function SyncProblemAlert({ status }: { status: SyncStatus }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { date } = useFormat();
  const { syncNow } = useDeviceSyncMutations();
  const [open, setOpen] = useState<Open>(null);
  /** A repair that could not send every change: re-syncing is offered next to it. */
  const [offerResync, setOfferResync] = useState(false);
  /** The held change whose skip confirmation is open, as it was when the confirmation opened. */
  const [skipping, setSkipping] = useState<HeldGapView | null>(null);
  const heldGaps = useHeldGaps(status.problem === 'gap' || status.problem === 'clockAhead');
  const close = () => setOpen(null);
  const problem = status.problem;
  if (!problem) return null;
  // Out of reach for now: the engine retries, so no alert box (the status card shows a plain line)
  if (syncState(status, Math.floor(Date.now() / 1000))?.kind === 'unreachable') return null;

  const tryAgain = async () => {
    try {
      await syncNow.mutateAsync();
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  const now = Math.floor(Date.now() / 1000);
  const dateText = (seconds: number) => {
    const when = secondsToDate(seconds);
    return when ? date(when, { dateStyle: 'long' }) : '';
  };
  const held = heldFor(problem, heldGaps.data);
  // A held change younger than 14 days offers nothing: this device keeps syncing meanwhile
  const skippable = skippableGap(held, now);
  const action = problemAction(problem, status.ownServer, skippable);
  const deletesAt = secondsToDate(status.deletesAt);
  const ahead = problem === 'clockAhead' ? aheadGap(held) : null;
  let title: string | null = null;
  let body: string | null = ahead ? aheadText(ahead, t, dateText) : t(`status.problem.${problem}`);
  const skipFrom = firstSkippableAt(held, now);
  if (skipFrom !== null && dateText(skipFrom)) {
    const key = problem === 'clockAhead' ? 'alert.clockAhead.skipFrom' : 'alert.gap.skipFrom';
    body = `${body} ${t(key, { date: dateText(skipFrom) })}`;
  }
  let actions: React.ReactNode = null;
  switch (action) {
    case 'renew':
    case 'askOperator':
      title = status.ownServer ? t('alert.lapsed.titleOwnServer') : t('alert.lapsed.title');
      body =
        action === 'askOperator'
          ? t('alert.lapsed.selfHosted')
          : deletesAt
            ? t('alert.lapsed.deletes', { date: date(deletesAt, { dateStyle: 'long' }) })
            : null;
      if (action === 'renew') {
        actions = (
          <Button type="button" size="sm" onClick={() => setOpen('renew')}>
            {t('alert.lapsed.renew')}
          </Button>
        );
      }
      break;
    case 'turnOffOrRetry':
      body = t('alert.vaultGone.text');
      actions = (
        <>
          <Button type="button" size="sm" onClick={() => setOpen('turnOff')}>
            {t('alert.vaultGone.turnOff')}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={tryAgain}
            loading={syncNow.isPending}
          >
            {t('alert.vaultGone.retry')}
          </Button>
        </>
      );
      break;
    case 'repair':
      actions = (
        <>
          <Button type="button" size="sm" onClick={() => setOpen('repair')}>
            {t('repair.button')}
          </Button>
          {offerResync && (
            <Button type="button" size="sm" variant="outline" onClick={() => setOpen('resync')}>
              {t('resync.button')}
            </Button>
          )}
        </>
      );
      break;
    case 'updateApp':
      title = t('alert.newerSchema');
      body = null;
      break;
    case 'skipMissing':
      if (skippable?.kind === 'ahead') {
        body = aheadText(skippable, t, dateText);
        actions = (
          <Button type="button" size="sm" onClick={() => setSkipping(skippable)}>
            {t('alert.clockAhead.skip')}
          </Button>
        );
      } else if (skippable) {
        const device = heldGapDevice(skippable);
        const days = heldDays(skippable, now);
        body =
          skippable.kind === 'parent'
            ? t('alert.gap.waiting', { device, days })
            : t('alert.gap.missing', { device, days, count: skippable.missingChanges });
        actions = (
          <Button type="button" size="sm" onClick={() => setSkipping(skippable)}>
            {t('alert.gap.skip', { count: skippable.missingChanges })}
          </Button>
        );
      }
      break;
    case 'none':
      break;
    default: {
      // A new SyncProblemAction must be placed above: this fails to compile until it is
      const unhandled: never = action;
      return unhandled;
    }
  }

  return (
    <>
      <Alert variant={status.phase === 'error' ? 'destructive' : 'default'}>
        <AlertTriangle />
        {title && <AlertTitle>{title}</AlertTitle>}
        {body && <AlertDescription>{body}</AlertDescription>}
        {actions && <AlertActions>{actions}</AlertActions>}
      </Alert>
      {open === 'renew' && <SyncRenewDialog onClose={close} />}
      {open === 'repair' && (
        <RepairDeviceDialog onClose={close} onDrainIncomplete={() => setOfferResync(true)} />
      )}
      {open === 'resync' && <ResyncDeviceDialog onClose={close} />}
      {open === 'turnOff' && <TurnOffSyncDialog onClose={close} />}
      {skipping && <SkipMissingChangeDialog gap={skipping} onClose={() => setSkipping(null)} />}
    </>
  );
}
