import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { TriangleAlert } from 'lucide-react';
import type { SyncStatus } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { kitRowState, kitStorageKey } from '@/utils/sync-kit';
import { removalAvailable } from '@/utils/sync-removal';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { SectionHead } from '@/mobile/components/SectionHead';
import { CheckKitSheet } from '@/mobile/components/sync/CheckKitSheet';
import { NewKitSheet } from '@/mobile/components/sync/NewKitSheet';

/** The 48 pt buttons under the card. */
const BUTTON =
  'h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-2 text-center text-m-row';

/**
 * The recovery kit in Nastavení → Sync (spec §6.3), the phone's counterpart of the desktop's `KitRow`:
 * when it was made and where it is kept, when the server says it was last read ("Vím o tom" quiets a
 * read nobody explains), "Ověřit sadu" and "Vytvořit novou…". A kit the person never kept
 * ("neuložená") and no usable kit at all both lead to the new-kit sheet, which makes a kit when none
 * exists. A new kit is a key change, so it waits while another one of this device is finishing.
 */
export function KitSection({ status }: { status: SyncStatus }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { date } = useFormat();
  const { dismissNotice } = useDeviceSyncMutations();
  const [checking, setChecking] = useState(false);
  const [making, setMaking] = useState(false);
  const kit = status.recoveryKit;
  const state = kitRowState(kit);
  const available = removalAvailable(status);

  const day = (seconds: number) => date(new Date(seconds * 1000), { dateStyle: 'long' });
  const dismissUsed = async () => {
    try {
      await dismissNotice.mutateAsync('kitUsed');
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  const text =
    state === 'none'
      ? t('kit.none')
      : state === 'unsaved'
        ? t('kit.unsaved')
        : kit?.createdBy
          ? t('kit.created', { date: day(kit.createdAt), name: kit.createdBy })
          : kit
            ? t('kit.createdNoName', { date: day(kit.createdAt) })
            : '';

  return (
    <>
      <SectionHead title={t('kit.title')} />
      {status.kitUsed !== null && (
        <InlineAlert
          icon={TriangleAlert}
          action={[
            ...(available ? [{ label: t('kit.makeNewKit'), onClick: () => setMaking(true) }] : []),
            {
              label: t('kit.usedKnown'),
              onClick: () => void dismissUsed(),
              disabled: dismissNotice.isPending,
            },
          ]}
        >
          <span data-testid="kit-used-notice">{t('kit.used', { date: day(status.kitUsed) })}</span>
        </InlineAlert>
      )}
      <p className="mx-0.5 mb-2.5 mt-0 text-m-lead text-ink-2">{text}</p>
      {kit && kit.storage !== null && (
        <p className="mx-0.5 mb-1 mt-0 text-m-sub text-ink-3">
          {t('kit.keptLine', { where: t(kitStorageKey(kit.storage)) })}
        </p>
      )}
      {kit && kit.lastReadAt !== null && (
        <p className="mx-0.5 mb-1 mt-0 text-m-sub text-ink-3">
          {t('kit.lastUsed', { date: day(kit.lastReadAt) })}
        </p>
      )}
      <div className="mt-2.5 grid gap-2.5">
        {state === 'kept' && (
          <Button variant="secondary" className={BUTTON} onClick={() => setChecking(true)}>
            {t('kit.check')}
          </Button>
        )}
        <Button
          variant={state === 'kept' ? 'secondary' : 'default'}
          className={BUTTON}
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
      {/* Mounted only while open: unmounting forgets the typed code and a new kit */}
      {checking && <CheckKitSheet onClose={() => setChecking(false)} />}
      {making && <NewKitSheet missing={state === 'none'} onClose={() => setMaking(false)} />}
    </>
  );
}
