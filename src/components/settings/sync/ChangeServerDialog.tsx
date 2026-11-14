import { useEffect, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { useDeviceSyncStatus, useSyncDevices } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useServerChoice } from '@/hooks/use-server-choice';
import { ServerChoice } from '@/components/settings/sync/ServerChoice';
import { SyncPasswordDialog } from '@/components/settings/sync/SyncPasswordDialog';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { isAuthApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { isUnchangedAddress } from '@/utils/sync-setup';
import { otherDeviceNames } from '@/utils/sync-devices';

/**
 * Settings → Sync, "Change address": the new address (or a connection code, whose server is
 * used), checked like in setup; Continue asks for the device password and the change runs. Only
 * the same server can be moved to (the backend proves it). A refusal that is not about the password
 * (another server, unreachable, blocked …) returns to the address step with the message shown
 * there and the typed address kept; only a wrong password stays on the password step. Mounted only
 * while open; the typed password is a mutation variable and is dropped with the dialog.
 */
export function ChangeServerDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { changeServerAddress } = useDeviceSyncMutations();
  const server = useServerChoice('addressOrCode', 'server');
  const { data: status } = useDeviceSyncStatus();
  const { data: devices } = useSyncDevices();
  const [asking, setAsking] = useState(false);
  const [done, setDone] = useState(false);
  /** The refusal that sent the person back to the address step, for the address it was given for. */
  const [refusal, setRefusal] = useState<{ input: string; message: string } | null>(null);

  const reset = changeServerAddress.reset;
  useEffect(() => reset, [reset]);

  const next = (event: FormEvent) => {
    event.preventDefault();
    if (server.ready && !unchanged) setAsking(true);
  };

  // The address the server answered for: the normalized one
  const address = server.check?.address ?? '';
  // The same address: said here, before any password is asked for
  const unchanged = server.ready && isUnchangedAddress(address, status?.serverAddress ?? null);

  if (done) {
    const names = otherDeviceNames(devices);
    return (
      <Dialog open onOpenChange={(open) => !open && onClose()}>
        <DialogContent className="max-w-[440px]">
          <DialogHeader>
            <DialogTitle>{t('server.success')}</DialogTitle>
            <DialogDescription>
              {names.length > 0
                ? t('server.repeatOn', { devices: names.join(', ') })
                : t('server.repeatOnOthers')}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" onClick={onClose}>
              {tc('buttons.close')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  if (asking) {
    return (
      <SyncPasswordDialog
        title={t('server.dialogTitle')}
        description={t('server.dialogBody')}
        confirmLabel={t('server.change')}
        closeOnSuccess={false}
        onSubmit={async (password) => {
          try {
            await changeServerAddress.mutateAsync({ password, address });
          } catch (error) {
            if (isAuthApiError(error)) throw error;
            setRefusal({ input: server.input, message: translateApiError(error as Error, tc) });
            setAsking(false);
            return;
          }
          setDone(true);
          setAsking(false);
        }}
        onClose={onClose}
      />
    );
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-[560px]">
        <form onSubmit={next} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{t('server.dialogTitle')}</DialogTitle>
            <DialogDescription>{t('server.dialogBody')}</DialogDescription>
          </DialogHeader>
          <ServerChoice state={server} id="change-server" showCloud={false} />
          {unchanged && (
            <p className="m-0 text-micro font-500 text-ink-3" role="status">
              {t('server.unchanged')}
            </p>
          )}
          {refusal && refusal.input === server.input && (
            <p className="m-0 text-micro font-500 text-loss" role="alert">
              {refusal.message}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              {tc('buttons.cancel')}
            </Button>
            <Button type="submit" disabled={!server.ready || unchanged}>
              {tc('buttons.continue')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
