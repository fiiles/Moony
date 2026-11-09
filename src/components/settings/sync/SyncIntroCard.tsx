import { useTranslation } from 'react-i18next';
import { SettingsCard } from '@/components/settings/SettingsCard';
import { Button } from '@/components/ui/button';

interface SyncIntroCardProps {
  /** `settingUp`: a setup was started and not finished; the button resumes it. */
  resuming: boolean;
  onStart: () => void;
}

/** Settings → Sync while sync is off (or half set up): what it is, and the button that starts setup. */
export function SyncIntroCard({ resuming, onStart }: SyncIntroCardProps) {
  const { t } = useTranslation('deviceSync');
  return (
    <SettingsCard title={t('title')} description={t('description')}>
      <div className="grid gap-3 text-body">
        <p className="m-0 text-ink-2">{t('intro.description')}</p>
        <p className="m-0 text-micro font-500 text-ink-4">{t('intro.privacy')}</p>
        <div>
          <Button type="button" onClick={onStart}>
            {resuming ? t('intro.resume') : t('intro.start')}
          </Button>
        </div>
      </div>
    </SettingsCard>
  );
}
