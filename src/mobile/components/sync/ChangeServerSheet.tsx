import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useDeviceSyncStatus, useSyncDevices } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useServerChoice } from '@/hooks/use-server-choice';
import { isAuthApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { isUnchangedAddress } from '@/utils/sync-setup';
import { otherDeviceNames } from '@/utils/sync-devices';
import { Sheet } from '@/mobile/components/Sheet';
import { ServerChoiceBlock } from '@/mobile/components/sync/ServerChoiceBlock';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';
import { useListText } from '@/mobile/hooks/use-list-text';

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
  const list = useListText();
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

  const address = server.check?.address ?? '';
  // The same address: said here, before any password is asked for
  const unchanged = server.ready && isUnchangedAddress(address, status?.serverAddress ?? null);

  if (done) {
    const names = otherDeviceNames(devices);
    return (
      <Sheet
        open
        onOpenChange={(open) => !open && onClose()}
        title={t('server.success')}
        description={
          names.length > 0
            ? t('server.repeatOn', { devices: list(names) })
            : t('server.repeatOnOthers')
        }
        footer={
          <Button className={`${BUTTON} flex-1`} onClick={onClose}>
            {tc('buttons.close')}
          </Button>
        }
      >
        <></>
      </Sheet>
    );
  }

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
          try {
            await changeServerAddress.mutateAsync({ password, address });
          } catch (error) {
            if (isAuthApiError(error)) throw error;
            setRefusal({ input: server.input, message: translateApiError(error as Error, tc) });
            setAsking(false);
            return;
          }
          // The done screen keeps the sheet mounted: the typed password leaves the mutation now
          changeServerAddress.reset();
          setDone(true);
          setAsking(false);
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
            disabled={!server.ready || unchanged}
            onClick={() => setAsking(true)}
          >
            {tc('buttons.continue')}
          </Button>
        </>
      }
    >
      <ServerChoiceBlock state={server} />
      {unchanged && (
        <p className="m-0 mt-2 text-m-sub text-ink-3" role="status">
          {t('server.unchanged')}
        </p>
      )}
      {refusal && refusal.input === server.input && (
        <p className="m-0 mt-2 text-m-sub text-loss" role="alert">
          {refusal.message}
        </p>
      )}
    </Sheet>
  );
}
