import { useTranslation } from 'react-i18next';
import { useDevicePairing } from '@/hooks/use-device-pairing';
import { useHolderFinish } from '@/hooks/use-device-sync';
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
  const { view, error, isError, start, confirm, confirmError, cancel } = useDevicePairing();
  const finish = useHolderFinish(view);

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
        needsServer
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

  // A wrong or momentary answer shows under the field (`confirmError`); one that ended the pairing
  // is `error`, and the dialog offers to start again.
  const handleConfirm = (typedCode?: string) => {
    confirm.mutate(typedCode);
  };

  // Back to the password step for a fresh code.
  const startAgain = async () => {
    try {
      await cancel();
    } catch {
      // Gone on this device either way.
    }
  };

  const finished = view.stage === 'sent' || isError || view.contested;
  const closing = view.stage === 'sent' || isError;
  return (
    <Dialog open onOpenChange={(open) => !open && void close()}>
      <DialogContent className="max-w-[560px]">
        <DialogHeader>
          <DialogTitle>{t('pairing.addTitle')}</DialogTitle>
          <DialogDescription>
            {view.stage === 'compare' && !isError
              ? t('pairing.typeTitle')
              : t('pairing.addDescription')}
          </DialogDescription>
        </DialogHeader>
        <PairingSteps
          view={view}
          scanHint={t('pairing.scanHint')}
          error={isError ? (error as Error) : null}
          confirmError={confirmError}
          onConfirm={handleConfirm}
          confirming={confirm.isPending}
          finish={finish}
        />
        <DialogFooter>
          {isError && (
            <Button type="button" onClick={() => void startAgain()}>
              {t('pairing.startAgain')}
            </Button>
          )}
          <Button
            type="button"
            variant={finish.joined ? 'default' : finished ? 'outline' : 'ghost'}
            onClick={() => void close()}
          >
            {finish.joined
              ? tc('buttons.done')
              : closing
                ? tc('buttons.close')
                : tc('buttons.cancel')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
