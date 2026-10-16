import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { Segmented } from '@/components/ui/segmented';
import { authApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useTheme } from '@/theme/ThemeProvider';
import type { ThemePreference } from '@/utils/theme';

/**
 * Settings → General → "Vzhled": System / Light / Dark. The theme applies on click and the
 * profile is saved in the background without a success toast; a failed save reverts the theme
 * and shows the usual error toast. The profile wins over the device mirror after unlock
 * (the sync effect in ThemeProvider).
 */
export function AppearanceCard() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { preference, setPreference } = useTheme();
  const queryClient = useQueryClient();

  const save = useMutation({
    mutationFn: async ({ theme }: { theme: ThemePreference; previous: ThemePreference }) => {
      await authApi.updateProfile({ theme });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['user-profile'] });
    },
    onError: (error: Error, { previous }) => {
      setPreference(previous);
      toast.error(t('toast.updateFailed'), { description: translateApiError(error, tc) });
    },
  });

  const choose = (next: ThemePreference) => {
    if (next === preference) return;
    const previous = preference;
    setPreference(next);
    save.mutate({ theme: next, previous });
  };

  return (
    <SettingsCard title={t('appearance.title')} description={t('appearance.description')}>
      <SettingsRow label={t('appearance.mode')}>
        <Segmented
          value={preference}
          onValueChange={choose}
          aria-label={t('appearance.mode')}
          options={[
            { value: 'system', label: t('appearance.system') },
            { value: 'light', label: t('appearance.light') },
            { value: 'dark', label: t('appearance.dark') },
          ]}
        />
      </SettingsRow>
    </SettingsCard>
  );
}
