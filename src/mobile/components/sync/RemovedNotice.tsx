import { useState } from 'react';
import { useLocation } from 'wouter';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Info } from 'lucide-react';
import type { SyncRemovedNotice } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { sentences } from '@/utils/sentences';
import { removedNoticeKeys } from '@/utils/sync-notices';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { Sheet } from '@/mobile/components/Sheet';
import { leaveAfterSheet } from '@/mobile/hooks/use-leave-after-sheet';
import { joinChoiceNavigation } from '@/mobile/lib/join-choice';

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

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

/** "Rozumím" dismisses the notice; "Připojit znovu" dismisses it and then runs `then` (the join choice). */
function useRemovedActions() {
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
  return { pending: dismissNotice.isPending, run };
}

/**
 * Nastavení → Sync while sync is off on a device another one removed (or that left): the notice
 * stays until "Rozumím", and "Připojit znovu" opens the join choice.
 */
export function RemovedBlock({
  notice,
  onRejoin,
}: {
  notice: SyncRemovedNotice;
  onRejoin: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { locale } = useFormat();
  const text = useRemovedText(notice);
  const actions = useRemovedActions();
  return (
    <InlineAlert
      icon={Info}
      title={text.title}
      action={[
        {
          label: t('problem.rejoin.button'),
          onClick: () => void actions.run(onRejoin),
          disabled: actions.pending,
        },
        {
          label: t('removed.understood'),
          onClick: () => void actions.run(),
          disabled: actions.pending,
        },
      ]}
    >
      <span data-testid="removed-notice-card">{sentences(locale, [text.body, text.unsent])}</span>
    </InlineAlert>
  );
}

/**
 * Once at unlock: a sheet tells the person this device was removed from sync (readable with sync
 * off). Closing it any other way than a button keeps the notice (Nastavení → Sync still shows it)
 * and does not ask again until the next unlock. "Připojit znovu" dismisses the notice and opens
 * Nastavení → Sync with the join choice. Mounted in the shell while unlocked.
 */
export function RemovedNoticeSheet() {
  const { t } = useTranslation('deviceSync');
  const { data: status } = useDeviceSyncStatus();
  const [, navigate] = useLocation();
  const notice = status?.removedNotice ?? null;
  const [closed, setClosed] = useState(false);
  // An answered notice is gone: a later removal in the same unlock is told again
  if (status !== undefined && notice === null && closed) setClosed(false);
  // The text of the last notice shown: an answer clears the notice while the sheet still slides
  // away, and its sentences must not turn into a blank name and 1 January 1970 meanwhile.
  const [shown, setShown] = useState<SyncRemovedNotice | null>(notice);
  if (notice !== null && notice !== shown) setShown(notice);
  const text = useRemovedText(notice ?? shown ?? { by: '', at: 0, unsent: 0, left: false });
  const actions = useRemovedActions();

  return (
    <Sheet
      open={notice !== null && !closed}
      onOpenChange={(open) => {
        if (!open) setClosed(true);
      }}
      title={text.title}
      description={text.body}
      footer={
        <>
          <Button
            variant="ghost"
            className={`${BUTTON} px-[18px]`}
            loading={actions.pending}
            onClick={() => void actions.run()}
          >
            {t('removed.understood')}
          </Button>
          <Button
            className={`${BUTTON} flex-1`}
            disabled={actions.pending}
            onClick={() =>
              void actions.run(() =>
                // The sheet's own history entry goes first: a push before it would race its pop
                leaveAfterSheet(
                  () => setClosed(true),
                  () => {
                    const next = joinChoiceNavigation(
                      window.location.pathname,
                      window.history.state
                    );
                    navigate(next.to, { replace: next.replace, state: next.state });
                  }
                )
              )
            }
          >
            {t('problem.rejoin.button')}
          </Button>
        </>
      }
    >
      {text.unsent && <p className="m-0 text-m-lead text-ink-2">{text.unsent}</p>}
    </Sheet>
  );
}
