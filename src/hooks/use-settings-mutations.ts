import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { MenuPreferences, UserProfile } from '@shared/schema';
import { authApi } from '@/lib/tauri-api';
import { queryClient } from '@/lib/queryClient';
import { translateApiError } from '@/lib/translate-api-error';

export type ProfileUpdate = Partial<
  Pick<
    UserProfile,
    'name' | 'surname' | 'email' | 'currency' | 'language' | 'excludePersonalRealEstate'
  > & { menuPreferences: MenuPreferences }
>;

/**
 * Profile writes used by the Settings cards. Moved out of the old single
 * Settings page unchanged, except the language toast: a language
 * change says "Language changed" in the NEW language, built from a translator
 * fixed to that language so it cannot race the i18n switch.
 */
export function useSettingsMutations() {
  const { t, i18n } = useTranslation('settings');
  const { t: tc } = useTranslation('common');

  const updateProfile = useMutation({
    mutationFn: async (data: ProfileUpdate) => {
      await authApi.updateProfile(data);
    },
    onSuccess: (_data, variables) => {
      // Write the saved fields into the cache first: I18nProvider syncs the UI
      // language from `user.language`, and until the refetch lands it would
      // see the old value and flip the UI back to the old language.
      queryClient.setQueryData<UserProfile | null>(['user-profile'], (old) =>
        old ? { ...old, ...variables } : old
      );
      queryClient.invalidateQueries({ queryKey: ['user-profile'] });
      // Invalidate portfolio metrics if excludePersonalRealEstate changed
      if (variables && 'excludePersonalRealEstate' in variables) {
        queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      }
      if (variables?.language) {
        toast(i18n.getFixedT(variables.language, 'settings')('toast.languageChanged'));
      } else {
        toast(t('toast.profileUpdated'));
      }
    },
    onError: (error: Error) => {
      toast.error(t('toast.updateFailed'), { description: translateApiError(error, tc) });
    },
  });

  const updateMenuPreferences = useMutation({
    mutationFn: async (menuPreferences: MenuPreferences) => {
      await authApi.updateProfile({ menuPreferences });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['user-profile'] });
      toast(t('toast.preferencesUpdated'));
    },
    onError: (error: Error) => {
      toast.error(t('toast.updateFailed'), { description: translateApiError(error, tc) });
    },
  });

  return { updateProfile, updateMenuPreferences };
}
