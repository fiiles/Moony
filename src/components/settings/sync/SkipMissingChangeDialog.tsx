import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { HeldGapView } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { heldGapDevice } from '@/utils/device-sync-problem';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';

/**
 * Skip a change from another device that never arrived (audit M3): the confirmation names the
 * device and says what stays missing, then the password. A refusal (a wrong password, a change not
 * yet 14 days old) shows under the field.
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
  return (
    <SyncPasswordDialog
      title={t('skipGap.title')}
      description={t(gap.kind === 'parent' ? 'skipGap.descriptionParent' : 'skipGap.description', {
        device,
      })}
      confirmLabel={t('skipGap.confirm')}
      destructive
      onSubmit={async (password) => {
        await skipMissingChange.mutateAsync({ password, deviceId: gap.deviceId });
        toast(t('skipGap.success'));
      }}
      onClose={onClose}
    />
  );
}
