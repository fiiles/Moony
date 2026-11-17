import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useReplaceImpact } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { backupFileName } from '@/utils/sync-status';
import { consentOf } from '@/utils/replace-impact';
import { ReplaceImpactNotice } from '@/components/settings/sync/ReplaceImpactNotice';
import {
  IMPACT_CHANGED_KEY,
  replaceOutcomeToast,
} from '@/components/settings/sync/replace-outcome';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';

/** The key the backend sends when a repair could not send every change and replaced nothing. */
export const DRAIN_INCOMPLETE_KEY = 'validation.syncRepairDrainIncomplete';

/**
 * Repair a device whose change tracking is damaged (password needed; after a backup, replaces its
 * data). The dialog stays while the repair runs; a failure shows under the field.
 */
export function RepairDeviceDialog({
  onClose,
  onDrainIncomplete,
}: {
  onClose: () => void;
  /** The repair stopped because not every change could be sent: re-syncing is the way on. */
  onDrainIncomplete: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { repairDevice } = useDeviceSyncMutations();
  // The password is the mutation's variable: it leaves the cache with the dialog.
  const resetRepair = repairDevice.reset;
  useEffect(() => resetRepair, [resetRepair]);
  const impact = useReplaceImpact();
  return (
    <SyncPasswordDialog
      needsServer
      title={t('repair.title')}
      description={t('repair.description')}
      extra={
        <ReplaceImpactNotice
          flow="repair"
          impact={impact.data}
          error={impact.isError ? (impact.error as Error) : null}
        />
      }
      submitDisabled={!impact.data || impact.isFetching}
      confirmLabel={t('repair.confirm')}
      destructive
      onSubmit={async (password) => {
        try {
          const outcome = await repairDevice.mutateAsync(password);
          replaceOutcomeToast(
            t,
            t('repair.success', { file: backupFileName(outcome.backupPath) }),
            outcome
          );
        } catch (error) {
          if (error instanceof ApiError && error.key === DRAIN_INCOMPLETE_KEY) onDrainIncomplete();
          throw error;
        }
      }}
      onClose={onClose}
    />
  );
}

/**
 * Re-sync this device: the confirmation shows what is at stake (loaded when it opens), then the
 * password; the backend gets the consent of exactly the numbers shown. If they changed meanwhile,
 * the new ones replace them and the dialog stays.
 */
export function ResyncDeviceDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { resyncDevice } = useDeviceSyncMutations();
  const impact = useReplaceImpact();
  // The password is the mutation's variable: it leaves the cache with the dialog.
  const resetResync = resyncDevice.reset;
  useEffect(() => resetResync, [resetResync]);
  return (
    <SyncPasswordDialog
      needsServer
      title={t('resync.title')}
      description={t('resync.description')}
      extra={
        <ReplaceImpactNotice
          flow="resync"
          impact={impact.data}
          error={impact.isError ? (impact.error as Error) : null}
        />
      }
      submitDisabled={!impact.data || impact.isFetching}
      confirmLabel={t('resync.confirm')}
      destructive
      onSubmit={async (password) => {
        // The confirm is off until the numbers are in; this only narrows the type, and never lets
        // the dialog close without replacing
        const shown = impact.data;
        if (!shown) throw new Error('The numbers to confirm are not loaded.');
        try {
          const outcome = await resyncDevice.mutateAsync({
            password,
            consent: consentOf('resync', shown),
          });
          replaceOutcomeToast(
            t,
            t('resync.success', { file: backupFileName(outcome.backupPath) }),
            outcome
          );
        } catch (error) {
          if (error instanceof ApiError && error.key === IMPACT_CHANGED_KEY) {
            await impact.refetch();
          }
          throw error;
        }
      }}
      onClose={onClose}
    />
  );
}
