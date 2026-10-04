import { useTranslation } from 'react-i18next';
import { Switch } from '@/components/ui/switch';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useAuth } from '@/hooks/use-auth';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { menuToggleItems, type MenuPrefKey } from '@/components/common/nav-config';

/**
 * Menu visibility toggles (prototype `settings.html`, "Viditelnost v menu").
 * The rows come from the sidebar's own nav tree and reuse its `nav.*` labels,
 * so the two can never diverge. Stored preferences that still
 * carry the retired `savings` key are read and written back without it.
 */
export function MenuVisibilityCard() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { user } = useAuth();
  const { updateMenuPreferences } = useSettingsMutations();

  const handleToggle = (key: MenuPrefKey, checked: boolean) => {
    if (!user?.menuPreferences) return;
    updateMenuPreferences.mutate({ ...user.menuPreferences, [key]: checked });
  };

  return (
    <SettingsCard title={t('menuVisibility.title')} description={t('menuVisibility.description')}>
      {menuToggleItems().map((item) => (
        <SettingsRow key={item.id} htmlFor={`menu-${item.id}`} label={tc(item.labelKey)}>
          <Switch
            id={`menu-${item.id}`}
            checked={user?.menuPreferences?.[item.prefKey] ?? true}
            onCheckedChange={(checked) => handleToggle(item.prefKey, checked)}
          />
        </SettingsRow>
      ))}
    </SettingsCard>
  );
}
