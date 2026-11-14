import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useServerChoice } from '@/hooks/use-server-choice';
import { isAuthApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { Sheet } from '@/mobile/components/Sheet';
import { ServerChoiceBlock } from '@/mobile/components/sync/ServerChoiceBlock';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';

const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * The phone's "Change address": the new address (or a connection code), checked like in setup;
 * Continue opens the device password sheet and the change runs. A refusal that is not about the
 * password (another server, unreachable, blocked …) returns to the address step with the message
 * shown there and the typed address kept; only a wrong password stays on the password sheet.
 * Mounted only while open (the parent drops it on close); the typed password is a mutation
 * variable and is dropped with it.
 */
export function ChangeServerSheet({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { changeServerAddress } = useDeviceSyncMutations();
  const server = useServerChoice('addressOrCode', 'server');
  const { data: status } = useDeviceSyncStatus();
  const [asking, setAsking] = useState(false);
  /** The refusal that sent the person back to the address step, for the address it was given for. */
  const [refusal, setRefusal] = useState<{ input: string; message: string } | null>(null);

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
        closeOnSuccess={false}
        onSubmit={async (password) => {
          // The address the device used before: the status is refreshed once the change is done
          const before = status?.serverAddress ?? null;
          try {
            await changeServerAddress.mutateAsync({ password, address });
          } catch (error) {
            if (isAuthApiError(error)) throw error;
            setRefusal({ input: server.input, message: translateApiError(error as Error, tc) });
            setAsking(false);
            return;
          }
          toast(address === before ? t('server.unchanged') : t('server.success'));
          onClose();
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
      {refusal && refusal.input === server.input && (
        <p className="m-0 mt-2 text-m-sub text-loss" role="alert">
          {refusal.message}
        </p>
      )}
    </Sheet>
  );
}
