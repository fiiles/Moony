import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { SettingsCard } from '@/components/settings/SettingsCard';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';

interface SyncIntroCardProps {
  /** `settingUp`: a setup was started and not finished; the button resumes it. */
  resuming: boolean;
  onStart: () => void;
  /** Join an existing sync account by pairing with a device that has sync on. */
  onJoinByPairing: () => void;
  /** Join an existing sync account with its Sync Recovery Kit. */
  onJoinWithKit: () => void;
}

/**
 * Settings → Sync while sync is off (or half set up): what it is, the button that starts setup and
 * two quieter ways to join an account that already exists. A half-finished setup can be cancelled
 * here (a join refuses to start next to one), after a confirmation: this device forgets the sync
 * account the setup created.
 */
export function SyncIntroCard({
  resuming,
  onStart,
  onJoinByPairing,
  onJoinWithKit,
}: SyncIntroCardProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { turnOff } = useDeviceSyncMutations();

  const [confirming, setConfirming] = useState(false);

  const cancelSetup = async () => {
    try {
      await turnOff.mutateAsync();
      setConfirming(false);
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  return (
    <SettingsCard title={t('title')} description={t('description')}>
      <div className="grid gap-3 text-body">
        <p className="m-0 text-ink-2">{t('intro.description')}</p>
        <p className="m-0 text-micro font-500 text-ink-4">{t('intro.privacy')}</p>
        <div className="flex flex-wrap gap-2">
          <Button type="button" onClick={onStart}>
            {resuming ? t('intro.resume') : t('intro.start')}
          </Button>
          <Button type="button" variant="outline" onClick={onJoinByPairing}>
            {t('join.introPair')}
          </Button>
          <Button type="button" variant="outline" onClick={onJoinWithKit}>
            {t('join.introKit')}
          </Button>
          {resuming && (
            <Button type="button" variant="ghost" onClick={() => setConfirming(true)}>
              {t('intro.cancelSetup')}
            </Button>
          )}
        </div>
      </div>
      <AlertDialog
        open={confirming}
        onOpenChange={(next) => !turnOff.isPending && setConfirming(next)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('intro.cancelConfirm.title')}</AlertDialogTitle>
            <AlertDialogDescription>{t('intro.cancelConfirm.description')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={turnOff.isPending}>
              {t('intro.cancelConfirm.keep')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={(event) => {
                // Stays open until the setup is cancelled, so a failure leaves the dialog and toast
                event.preventDefault();
                void cancelSetup();
              }}
              disabled={turnOff.isPending}
            >
              {t('intro.cancelSetup')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SettingsCard>
  );
}
