import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Info } from 'lucide-react';
import type { SyncRemovedNotice } from '@shared/schema';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { removedNoticeKeys } from '@/utils/sync-notices';
import { JoinDeviceDialog } from '@/components/settings/sync/join/JoinDeviceDialog';
import { WRAP_BUTTON } from '@/components/settings/sync/wrap-button';
import { Alert, AlertActions, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

/** The removed notice's sentences: who removed this device and when (or that it left), and the changes that never left it. */
function useRemovedText(notice: SyncRemovedNotice) {
  const { t } = useTranslation('deviceSync');
  const { date } = useFormat();
  const keys = removedNoticeKeys(notice);
  return {
    title: t(keys.titleKey),
    body: t(keys.bodyKey, {
      by: notice.by,
      date: date(new Date(notice.at * 1000), { dateStyle: 'long' }),
    }),
    unsent: keys.showUnsent ? t('removed.unsent', { count: notice.unsent }) : null,
  };
}

/** "Rozumím" and "Připojit znovu": the first dismisses the notice, the second dismisses it and opens the join. */
function useRemovedActions(onRejoin: () => void) {
  const { t: tc } = useTranslation('common');
  const { dismissNotice } = useDeviceSyncMutations();
  const run = async (then?: () => void) => {
    try {
      await dismissNotice.mutateAsync('removed');
      then?.();
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };
  return {
    pending: dismissNotice.isPending,
    understood: () => void run(),
    rejoin: () => void run(onRejoin),
  };
}

/**
 * Settings → Sync while sync is off on a device another one removed (or that left): the notice
 * stays until "Rozumím", and "Připojit znovu" opens the join choice.
 */
export function RemovedNoticeCard({
  notice,
  onRejoin,
}: {
  notice: SyncRemovedNotice;
  onRejoin: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const text = useRemovedText(notice);
  const actions = useRemovedActions(onRejoin);
  return (
    <Alert data-testid="removed-notice-card">
      <Info />
      <AlertTitle>{text.title}</AlertTitle>
      <AlertDescription className="break-words">
        {text.body}
        {text.unsent && <span className="mt-1 block">{text.unsent}</span>}
      </AlertDescription>
      <AlertActions className="flex-wrap">
        <Button
          type="button"
          size="sm"
          className={WRAP_BUTTON}
          disabled={actions.pending}
          onClick={actions.rejoin}
        >
          {t('problem.rejoin.button')}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className={WRAP_BUTTON}
          loading={actions.pending}
          onClick={actions.understood}
        >
          {t('removed.understood')}
        </Button>
      </AlertActions>
    </Alert>
  );
}

/**
 * Once at unlock: a dialog tells the person this device was removed from sync (readable with sync
 * off). Closing it any other way than a button keeps the notice (Settings and the top-bar popover
 * still show it) and does not ask again until the next unlock. Mounted in the app shell.
 */
export function RemovedNoticeDialog() {
  const { t } = useTranslation('deviceSync');
  const { data: status } = useDeviceSyncStatus();
  const notice = status?.removedNotice ?? null;
  const [closed, setClosed] = useState(false);
  const [joining, setJoining] = useState(false);
  const text = useRemovedText(notice ?? { by: '', at: 0, unsent: 0, left: false });
  const actions = useRemovedActions(() => setJoining(true));

  return (
    <>
      {notice && !closed && !joining && (
        <Dialog open onOpenChange={(open) => !open && setClosed(true)}>
          <DialogContent className="max-w-[460px]" data-testid="removed-notice-dialog">
            <DialogHeader>
              <DialogTitle>{text.title}</DialogTitle>
              <DialogDescription>{text.body}</DialogDescription>
            </DialogHeader>
            {text.unsent && <p className="m-0 text-body text-ink-2">{text.unsent}</p>}
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                className={WRAP_BUTTON}
                loading={actions.pending}
                onClick={actions.understood}
              >
                {t('removed.understood')}
              </Button>
              <Button
                type="button"
                className={WRAP_BUTTON}
                disabled={actions.pending}
                onClick={actions.rejoin}
              >
                {t('problem.rejoin.button')}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}
      {joining && <JoinDeviceDialog onClose={() => setJoining(false)} />}
    </>
  );
}
