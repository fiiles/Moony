import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { SettingsCard } from '@/components/settings/SettingsCard';
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
 * here (a join refuses to start next to one).
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

  const cancelSetup = async () => {
    try {
      await turnOff.mutateAsync();
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
            <Button type="button" variant="ghost" onClick={cancelSetup} loading={turnOff.isPending}>
              {t('intro.cancelSetup')}
            </Button>
          )}
        </div>
      </div>
    </SettingsCard>
  );
}
