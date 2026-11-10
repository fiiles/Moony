import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { RecoveryKit } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { Sheet } from '@/mobile/components/Sheet';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';
import { KitBody } from '@/mobile/components/sync/KitBody';

const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * Settings → Sync → Show Recovery Kit: the device password first, then the kit read-only (QR,
 * account, grouped code). The kit holds the vault key, so it is forgotten when the sheet closes,
 * from the state and from the mutation's result alike. Copying the code works only shortly after
 * the kit was shown; afterwards Rust asks for the password again and this goes back to the
 * password step.
 */
export function SyncKitSheet({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { showRecoveryKit } = useDeviceSyncMutations();
  const [kit, setKit] = useState<RecoveryKit | null>(null);
  const [again, setAgain] = useState(false);

  const resetShownKit = showRecoveryKit.reset;
  useEffect(() => resetShownKit, [resetShownKit]);
  useEffect(() => {
    if (!open) resetShownKit();
  }, [open, resetShownKit]);
  // Closed: the kit and the "again" sentence go with it
  const [wasOpen, setWasOpen] = useState(open);
  if (wasOpen !== open) {
    setWasOpen(open);
    if (!open) {
      setKit(null);
      setAgain(false);
    }
  }

  return (
    <>
      <SyncPasswordSheet
        open={open && kit === null}
        onOpenChange={onOpenChange}
        title={t('showKit.title')}
        description={again ? t('showKit.again') : t('showKit.description')}
        confirmLabel={t('showKit.confirm')}
        closeOnSuccess={false}
        onSubmit={async (password) => {
          setKit(await showRecoveryKit.mutateAsync(password));
        }}
      />
      <KitSheet
        kit={open ? kit : null}
        onOpenChange={onOpenChange}
        onPasswordRequired={() => {
          resetShownKit();
          setKit(null);
          setAgain(true);
        }}
      />
    </>
  );
}

function KitSheet({
  kit,
  onOpenChange,
  onPasswordRequired,
}: {
  kit: RecoveryKit | null;
  onOpenChange: (open: boolean) => void;
  onPasswordRequired: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');

  return (
    <Sheet
      open={kit !== null}
      onOpenChange={onOpenChange}
      title={t('showKit.title')}
      description={t('setup.kit.description')}
      footer={
        <Button className={`${BUTTON} flex-1`} onClick={() => onOpenChange(false)}>
          {tc('buttons.close')}
        </Button>
      }
    >
      {kit && <KitBody kit={kit} onPasswordRequired={onPasswordRequired} />}
    </Sheet>
  );
}
