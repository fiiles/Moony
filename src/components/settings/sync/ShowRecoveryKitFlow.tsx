import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { RecoveryKit } from '@shared/schema';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { RecoveryKitPanel } from '@/components/settings/sync/RecoveryKitPanel';
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
 * Settings → Sync → Show Recovery Kit: the device password first, then the kit read-only. The kit
 * is forgotten when this closes (also from the mutation's result). If copying the code needs the
 * password again, the kit goes away and the password is asked once more.
 */
export function ShowRecoveryKitFlow({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { showRecoveryKit } = useDeviceSyncMutations();
  const [kit, setKit] = useState<RecoveryKit | null>(null);
  const [again, setAgain] = useState(false);

  const resetShownKit = showRecoveryKit.reset;
  useEffect(() => resetShownKit, [resetShownKit]);

  if (!kit) {
    return (
      <SyncPasswordDialog
        title={t('showKit.title')}
        description={again ? t('showKit.again') : t('showKit.description')}
        confirmLabel={t('showKit.confirm')}
        closeOnSuccess={false}
        onSubmit={async (password) => {
          setKit(await showRecoveryKit.mutateAsync(password));
        }}
        onClose={onClose}
      />
    );
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-[560px]">
        <DialogHeader>
          <DialogTitle>{t('showKit.title')}</DialogTitle>
          <DialogDescription>{t('setup.kit.description')}</DialogDescription>
        </DialogHeader>
        <RecoveryKitPanel
          kit={kit}
          onKept={() => {}}
          onPasswordRequired={() => {
            showRecoveryKit.reset();
            setKit(null);
            setAgain(true);
          }}
        />
        <DialogFooter>
          <Button type="button" onClick={onClose}>
            {tc('buttons.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
