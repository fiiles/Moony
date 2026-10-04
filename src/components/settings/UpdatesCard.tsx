import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { UpdateStatusLine } from '@/components/common/UpdateStatusLine';
import { useUpdater } from '@/hooks/use-updater';

/**
 * Settings → Security → Updates: the device-level "check automatically" switch
 * and a manual check whose result shows in the row. Everything lives
 * in the shared UpdaterProvider; installing opens the same dialog as the badge.
 */
export function UpdatesCard() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { status, autoCheck, setAutoCheck, check, install } = useUpdater();

  const checking = status === 'checking';
  const busy = checking || status === 'installing';

  return (
    <SettingsCard title={t('updates.title')} description={t('updates.description')}>
      <SettingsRow
        htmlFor="auto-update-check"
        label={t('updates.autoCheck')}
        hint={t('updates.autoCheckHint')}
      >
        <Switch id="auto-update-check" checked={autoCheck} onCheckedChange={setAutoCheck} />
      </SettingsRow>
      <SettingsRow label={t('updates.manualCheck')} hint={<UpdateStatusLine />}>
        {status === 'available' && (
          <Button size="sm" onClick={() => void install()} disabled={busy}>
            {tc('update.install')}
          </Button>
        )}
        <Button
          variant="outline"
          size="sm"
          onClick={() => void check()}
          loading={checking}
          disabled={busy}
        >
          {t('updates.checkNow')}
        </Button>
      </SettingsRow>
    </SettingsCard>
  );
}
