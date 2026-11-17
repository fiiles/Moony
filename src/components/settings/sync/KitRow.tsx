import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { TriangleAlert } from 'lucide-react';
import type { SyncStatus } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { kitRowState, kitStorageKey } from '@/utils/sync-kit';
import { removalAvailable } from '@/utils/sync-removal';
import { CheckKitDialog } from '@/components/settings/sync/CheckKitDialog';
import { NewKitDialog } from '@/components/settings/sync/NewKitDialog';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';

/** Labels wrap instead of being cut (long German and Polish buttons). */
const WRAP_BUTTON = 'h-auto min-h-control whitespace-normal py-1.5 text-center';

/**
 * The recovery kit in Settings → Sync (spec §6.3): when it was made and where it is kept, when the
 * server says it was last read ("Vím o tom" quiets a read nobody explains), "Ověřit sadu" and
 * "Vytvořit novou…". A kit the person never kept ("neuložená") and no usable kit at all both lead to
 * the new-kit dialog, which makes a kit when none exists. A new kit is a key change, so it waits
 * while another one of this device is still finishing (`removalAvailable`).
 */
export function KitRow({ status }: { status: SyncStatus }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { date } = useFormat();
  const { dismissNotice } = useDeviceSyncMutations();
  const [checking, setChecking] = useState(false);
  const [making, setMaking] = useState(false);
  const kit = status.recoveryKit;
  const state = kitRowState(kit);
  const available = removalAvailable(status);

  const dismissUsed = async () => {
    try {
      await dismissNotice.mutateAsync('kitUsed');
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  const day = (seconds: number) => date(new Date(seconds * 1000), { dateStyle: 'long' });

  return (
    <>
      <div className="grid gap-3 border-b border-line-soft py-3 last:border-b-0">
        <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
          <div className="min-w-0 flex-1 basis-64">
            <b className="block text-body font-600 text-ink">{t('kit.title')}</b>
            <small className="mt-0.5 block text-caption text-ink-4" data-testid="kit-row-hint">
              {state === 'none' && t('kit.none')}
              {state === 'unsaved' && t('kit.unsaved')}
              {kit && state === 'kept' && (
                <>
                  {kit.createdBy
                    ? t('kit.created', { date: day(kit.createdAt), name: kit.createdBy })
                    : t('kit.createdNoName', { date: day(kit.createdAt) })}
                </>
              )}
            </small>
            {kit && kit.storage !== null && (
              <small className="mt-0.5 block text-caption text-ink-4">
                {t('kit.keptLine', { where: t(kitStorageKey(kit.storage)) })}
              </small>
            )}
            {kit && kit.lastReadAt !== null && (
              <small className="mt-0.5 block text-caption text-ink-4">
                {t('kit.lastUsed', { date: day(kit.lastReadAt) })}
              </small>
            )}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {state === 'kept' && (
              <Button
                type="button"
                variant="outline"
                className={WRAP_BUTTON}
                onClick={() => setChecking(true)}
              >
                {t('kit.check')}
              </Button>
            )}
            <Button
              type="button"
              variant={state === 'kept' ? 'outline' : 'default'}
              className={WRAP_BUTTON}
              disabled={!available}
              onClick={() => setMaking(true)}
            >
              {state === 'none'
                ? t('kit.make')
                : state === 'unsaved'
                  ? t('kit.makeNewKit')
                  : t('kit.makeNew')}
            </Button>
          </div>
        </div>
        {status.kitUsed !== null && (
          <Alert>
            <TriangleAlert />
            <AlertDescription className="grid gap-2">
              <span data-testid="kit-used-notice">
                {t('kit.used', { date: day(status.kitUsed) })}
              </span>
              <span className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  className={WRAP_BUTTON}
                  disabled={!available}
                  onClick={() => setMaking(true)}
                >
                  {t('kit.makeNewKit')}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  className={WRAP_BUTTON}
                  loading={dismissNotice.isPending}
                  onClick={dismissUsed}
                >
                  {t('kit.usedKnown')}
                </Button>
              </span>
            </AlertDescription>
          </Alert>
        )}
      </div>
      {checking && <CheckKitDialog onClose={() => setChecking(false)} />}
      {making && <NewKitDialog missing={state === 'none'} onClose={() => setMaking(false)} />}
    </>
  );
}
