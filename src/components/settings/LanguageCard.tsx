import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { LanguageSelect } from '@/components/common/LanguageSelect';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useLanguage } from '@/i18n/I18nProvider';
import type { SupportedLanguage } from '@/i18n/index';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { authApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import {
  FX_SUPPORTED_CODES,
  currencyName,
  isFxSupported,
  type CurrencyCode,
} from '@shared/currencies';

/**
 * Settings → General → "Jazyk a formáty": the UI language as a row with the
 * language menu (each language shows the example amount in its format), and
 * the main currency as a row with the searchable picker.
 */
export function LanguageCard() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { language, setLanguage } = useLanguage();
  const { updateProfile } = useSettingsMutations();
  const queryClient = useQueryClient();
  const fmt = useFormat();
  const { currencyCode, setCurrency } = useCurrency();

  // The UI language switches once the profile is saved: switching first made
  // I18nProvider flip it back to the stored (old) language until the refetch.
  const select = (lang: SupportedLanguage) => {
    if (lang === language) return;
    updateProfile.mutate({ language: lang }, { onSuccess: () => setLanguage(lang) });
  };

  const updateCurrency = useMutation({
    mutationFn: async (currency: CurrencyCode) => {
      await authApi.updateProfile({ currency });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['user-profile'] });
      toast(t('toast.profileUpdated'));
    },
    onError: (error: Error) => {
      toast.error(t('toast.updateFailed'), { description: translateApiError(error, tc) });
    },
  });

  const example = fmt.money(1234.56, currencyCode);
  const hasRateSource = isFxSupported(currencyCode);

  return (
    <SettingsCard title={t('language.title')}>
      <SettingsRow label={t('language.interface')} hint={t('language.description')}>
        <LanguageSelect
          variant="field"
          value={language}
          onChange={select}
          currency={currencyCode}
          disabled={updateProfile.isPending}
        />
      </SettingsRow>

      <SettingsRow
        label={t('currency.main')}
        hint={
          <>
            {t('currency.mainHint')} {currencyName(currencyCode, fmt.locale)} ({currencyCode}) ·{' '}
            {t('currency.example', { example })}
            <span className="mt-0.5 block">{t('currency.nativeHint')}</span>
            {!hasRateSource && (
              <span className="mt-0.5 block text-loss">{t('currency.unsupportedMain')}</span>
            )}
          </>
        }
      >
        <CurrencyCombobox
          value={currencyCode}
          onChange={(code) => {
            setCurrency(code);
            updateCurrency.mutate(code);
          }}
          options={FX_SUPPORTED_CODES}
          disabled={updateCurrency.isPending}
          aria-label={t('currency.main')}
          className="w-[200px]"
        />
      </SettingsRow>
    </SettingsCard>
  );
}
