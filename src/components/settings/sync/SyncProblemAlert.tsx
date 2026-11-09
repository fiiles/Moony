import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AlertTriangle } from 'lucide-react';
import type { SyncStatus } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { isSelfHostedPaidUntil } from '@/utils/sync-status';
import { Alert, AlertActions, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { SyncRenewDialog } from '@/components/settings/sync/SyncRenewDialog';
import {
  RepairDeviceDialog,
  ResyncDeviceDialog,
} from '@/components/settings/sync/SyncRepairDialogs';
import { TurnOffSyncDialog } from '@/components/settings/sync/TurnOffSyncDialog';

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
  const close = () => setOpen(null);
  const problem = status.problem;
  if (!problem) return null;

  const tryAgain = async () => {
    try {
      await syncNow.mutateAsync();
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  let title: string | null = null;
  let body: string | null = t(`status.problem.${problem}`);
  let actions: React.ReactNode = null;
  switch (problem) {
    case 'lapsed':
      title = t('alert.lapsed.title');
      body = isSelfHostedPaidUntil(status.paidUntil)
        ? t('alert.lapsed.selfHosted')
        : status.deletesAt !== null
          ? t('alert.lapsed.deletes', {
              date: date(new Date(status.deletesAt * 1000), { dateStyle: 'long' }),
            })
          : null;
      if (!isSelfHostedPaidUntil(status.paidUntil)) {
        actions = (
          <Button type="button" size="sm" onClick={() => setOpen('renew')}>
            {t('alert.lapsed.renew')}
          </Button>
        );
      }
      break;
    case 'vaultGone':
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
    case 'captureBroken':
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
    case 'newerSchema':
      title = t('alert.newerSchema');
      body = null;
      break;
    case 'gap':
      actions = (
        <Button type="button" size="sm" onClick={() => setOpen('resync')}>
          {t('resync.button')}
        </Button>
      );
      break;
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
    </>
  );
}
