import { useEffect, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
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

/**
 * Settings → Sync, "Change address": the new address (or a connection code, whose server is
 * used), checked like in setup; Continue asks for the device password and the change runs. Only
 * the same server can be moved to (the backend proves it). Mounted only while open; the typed
 * password is a mutation variable and is dropped with the dialog.
 */
export function ChangeServerDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { changeServerAddress } = useDeviceSyncMutations();
  const server = useServerChoice('addressOrCode', 'server');
  const [asking, setAsking] = useState(false);

  const reset = changeServerAddress.reset;
  useEffect(() => reset, [reset]);

  const next = (event: FormEvent) => {
    event.preventDefault();
    if (server.ready) setAsking(true);
  };

  // The address the server answered for: the normalized one
  const address = server.check?.address ?? '';

  if (asking) {
    return (
      <SyncPasswordDialog
        title={t('server.dialogTitle')}
        description={t('server.dialogBody')}
        confirmLabel={t('server.change')}
        onSubmit={async (password) => {
          await changeServerAddress.mutateAsync({ password, address });
          toast(t('server.success'));
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
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              {tc('buttons.cancel')}
            </Button>
            <Button type="submit" disabled={!server.ready}>
              {tc('buttons.continue')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
