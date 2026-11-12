import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { HeldGapView } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useFormat } from '@/lib/use-format';
import { secondsToDate } from '@/utils/sync-status';
import { skipGapTexts } from '@/utils/device-sync-problem';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';

/**
 * Skip a change from another device that never arrived (audit M3), or one dated more than ten
 * years ahead (audit M6): the confirmation names the device, how many changes go (or the date the
 * change carries) and what that costs, then the password. A refusal (a wrong
 * password, a change not yet 14 days old) shows under the field; a change that arrived meanwhile
 * is said so rather than "skipped".
 */
export function SkipMissingChangeDialog({
  gap,
  onClose,
}: {
  gap: HeldGapView;
  onClose: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { skipMissingChange } = useDeviceSyncMutations();
  // The password is the mutation's variable: it leaves the cache with the dialog.
  const resetSkip = skipMissingChange.reset;
  useEffect(() => resetSkip, [resetSkip]);
  const { date } = useFormat();
  const texts = skipGapTexts(gap, t, (seconds) => {
    const when = secondsToDate(seconds);
    return when ? date(when, { dateStyle: 'long' }) : '';
  });
  return (
    <SyncPasswordDialog
      title={texts.title}
      description={texts.description}
      confirmLabel={t('skipGap.confirm')}
      destructive
      onSubmit={async (password) => {
        const skipped = await skipMissingChange.mutateAsync({ password, deviceId: gap.deviceId });
        toast(skipped ? texts.skipped : texts.unchanged);
      }}
      onClose={onClose}
    />
  );
}
