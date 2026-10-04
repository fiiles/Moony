import { useTranslation } from 'react-i18next';
import { Switch } from '@/components/ui/switch';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useAuth } from '@/hooks/use-auth';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';

/** Settings → General → Preference přehledu: what the overview counts. */
export function DashboardPreferencesCard() {
  const { t } = useTranslation('settings');
  const { user } = useAuth();
  const { updateProfile } = useSettingsMutations();

  return (
    <SettingsCard
      title={t('dashboardPreferences.title')}
      description={t('dashboardPreferences.description')}
    >
      <SettingsRow
        htmlFor="exclude-personal-real-estate"
        label={t('dashboardPreferences.excludeRealEstate')}
        hint={t('dashboardPreferences.excludeRealEstateHint')}
      >
        <Switch
          id="exclude-personal-real-estate"
          checked={user?.excludePersonalRealEstate ?? false}
          onCheckedChange={(checked) =>
            updateProfile.mutate({ excludePersonalRealEstate: checked })
          }
        />
      </SettingsRow>
    </SettingsCard>
  );
}
