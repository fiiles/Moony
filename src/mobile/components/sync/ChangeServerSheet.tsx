import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useServerChoice } from '@/hooks/use-server-choice';
import { Sheet } from '@/mobile/components/Sheet';
import { ServerChoiceBlock } from '@/mobile/components/sync/ServerChoiceBlock';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';

const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * The phone's "Change address": the new address (or a connection code), checked like in setup;
 * Continue opens the device password sheet and the change runs. Mounted only while open (the
 * parent drops it on close); the typed password is a mutation variable and is dropped with it.
 */
export function ChangeServerSheet({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { changeServerAddress } = useDeviceSyncMutations();
  const server = useServerChoice('addressOrCode', 'server');
  const [asking, setAsking] = useState(false);

  const reset = changeServerAddress.reset;
  useEffect(() => reset, [reset]);

  const address = server.check?.address ?? '';

  if (asking) {
    return (
      <SyncPasswordSheet
        open
        onOpenChange={(open) => !open && onClose()}
        title={t('server.dialogTitle')}
        description={t('server.dialogBody')}
        confirmLabel={t('server.change')}
        onSubmit={async (password) => {
          await changeServerAddress.mutateAsync({ password, address });
          toast(t('server.success'));
        }}
      />
    );
  }

  return (
    <Sheet
      open
      onOpenChange={(open) => !open && onClose()}
      title={t('server.dialogTitle')}
      description={t('server.dialogBody')}
      dirty={server.input.trim() !== ''}
      footer={
        <>
          <Button variant="ghost" className={`${BUTTON} px-[18px]`} onClick={onClose}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            className={`${BUTTON} flex-1`}
            disabled={!server.ready}
            onClick={() => setAsking(true)}
          >
            {tc('buttons.continue')}
          </Button>
        </>
      }
    >
      <ServerChoiceBlock state={server} showCloud={false} />
    </Sheet>
  );
}
