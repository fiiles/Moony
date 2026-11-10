import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDevicePairing } from '@/hooks/use-device-pairing';
import { translateApiError } from '@/lib/translate-api-error';
import { PairingSteps } from '@/components/settings/sync/PairingSteps';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';

/**
 * Adding a device from this one (the holder): the device password first, then the QR, the six
 * digits to compare, and the hand-over. Mounted only while open; closing gives the pairing up.
 */
export function AddDeviceDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const status = useDeviceSyncStatus().data;
  const { view, error, isError, start, confirm, cancel } = useDevicePairing();

  const close = async () => {
    try {
      await cancel();
    } catch {
      // The pairing is gone on this device either way (the view is cleared in `cancel`).
    }
    onClose();
  };

  if (!view) {
    return (
      <SyncPasswordDialog
        title={t('pairing.addTitle')}
        description={t('pairing.passwordFirst')}
        confirmLabel={t('pairing.showCode')}
        closeOnSuccess={false}
        onSubmit={async (password) => {
          try {
            await start.mutateAsync({ password });
          } catch (failure) {
            start.reset();
            throw failure;
          }
        }}
        onClose={() => {
          start.reset();
          onClose();
        }}
      />
    );
  }

  const handleMatch = async () => {
    try {
      await confirm.mutateAsync();
    } catch (failure) {
      toast.error(tc('status.error'), { description: translateApiError(failure as Error, tc) });
    }
  };

  const finished = view.stage === 'sent' || isError || view.contested;
  return (
    <Dialog open onOpenChange={(open) => !open && void close()}>
      <DialogContent className="max-w-[480px]">
        <DialogHeader>
          <DialogTitle>{t('pairing.addTitle')}</DialogTitle>
          <DialogDescription>
            {view.stage === 'compare' ? t('pairing.compareTitle') : t('pairing.addDescription')}
          </DialogDescription>
        </DialogHeader>
        <PairingSteps
          view={view}
          scanHint={t('pairing.scanHint')}
          devTools={status?.devTools === true}
          error={isError ? (error as Error) : null}
          onMatch={handleMatch}
          matching={confirm.isPending}
          onNoMatch={() => void close()}
        />
        <DialogFooter>
          <Button
            type="button"
            variant={finished ? 'default' : 'ghost'}
            onClick={() => void close()}
          >
            {finished && !view.contested && !isError ? tc('buttons.close') : tc('buttons.cancel')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
