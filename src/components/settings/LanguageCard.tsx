import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useLanguage } from '@/i18n/I18nProvider';
import {
  SUPPORTED_LANGUAGES,
  LANGUAGE_NAMES,
  getLocaleForLanguage,
  type SupportedLanguage,
} from '@/i18n/index';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { authApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { getFormatters } from '@/lib/format';
import { translateApiError } from '@/lib/translate-api-error';
import { cn } from '@/lib/utils';
import { currencyName, isEcbCurrency, type CurrencyCode } from '@shared/currencies';

/** Tiles in the prototype's order: Čeština first. */
const LANGUAGE_ORDER: readonly SupportedLanguage[] = [
  'cs',
  ...SUPPORTED_LANGUAGES.filter((l) => l !== 'cs'),
];

/**
 * Settings → General → "Jazyk a formáty" (prototype `settings.html`): one
 * tile per language with the example amount in that language's format, and
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

  // Rows are <label>s, so a click reaches the radio exactly once. The UI
  // language switches once the profile is saved: switching first made
  // I18nProvider flip it back to the stored (old) language until the refetch.
  const select = (value: string) => {
    const lang = value as SupportedLanguage;
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
  const hasAutomaticRate = isEcbCurrency(currencyCode);

  return (
    <SettingsCard title={t('language.title')} description={t('language.description')}>
      <RadioGroup
        value={language}
        onValueChange={select}
        aria-label={t('language.title')}
        className="grid grid-cols-2 gap-[10px]"
      >
        {LANGUAGE_ORDER.map((lang) => {
          const active = lang === language;
          return (
            <label
              key={lang}
              htmlFor={`lang-${lang}`}
              className={cn(
                'flex cursor-pointer items-center gap-[10px] rounded-r2 border px-[13px] py-[11px] text-body font-600 text-ink transition-[border-color,box-shadow] duration-fast',
                active
                  ? 'border-dark shadow-[inset_0_0_0_1px_var(--dark)]'
                  : 'border-line-strong hover:border-line-hover'
              )}
            >
              <RadioGroupItem value={lang} id={`lang-${lang}`} />
              {LANGUAGE_NAMES[lang].native}
              <span className="ml-auto font-500 text-ink-4 num">
                {getFormatters(getLocaleForLanguage(lang)).money(1234.56, currencyCode)}
              </span>
            </label>
          );
        })}
      </RadioGroup>

      <SettingsRow
        className="mt-[14px]"
        label={t('currency.main')}
        hint={
          <>
            {t('currency.mainHint')} {currencyName(currencyCode, fmt.locale)} ({currencyCode}) ·{' '}
            {t('currency.example', { example })}
            {!hasAutomaticRate && (
              <span className="mt-0.5 block text-loss">{t('currency.noRateHint')}</span>
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
          disabled={updateCurrency.isPending}
          aria-label={t('currency.main')}
          className="w-[200px]"
        />
      </SettingsRow>
    </SettingsCard>
  );
}
