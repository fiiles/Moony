import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { HeldGapView } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { heldGapDevice } from '@/utils/device-sync-problem';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';

/**
 * Skip a change from another device that never arrived (audit M3): the confirmation names the
 * device, how many changes go and what that costs, then the password. A refusal (a wrong
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
  const device = heldGapDevice(gap);
  const count = gap.missingChanges;
  return (
    <SyncPasswordDialog
      title={t('skipGap.title', { count })}
      description={
        gap.kind === 'parent'
          ? t('skipGap.descriptionParent', { device })
          : t('skipGap.description', { device, count })
      }
      confirmLabel={t('skipGap.confirm')}
      destructive
      onSubmit={async (password) => {
        const skipped = await skipMissingChange.mutateAsync({ password, deviceId: gap.deviceId });
        toast(skipped ? t('skipGap.success', { count }) : t('skipGap.arrived'));
      }}
      onClose={onClose}
    />
  );
}
