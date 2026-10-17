import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { useLanguage } from '@/i18n/I18nProvider';
import { LANGUAGES, type SupportedLanguage } from '@/i18n/languages';
import { PickRow } from '@/mobile/components/PickRow';
import { Sheet } from '@/mobile/components/Sheet';

interface LanguagePickerSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * "Jazyk aplikace" (spec §8.9): one row per language in its own name, the current one checked.
 * A tap saves the profile first and switches the UI once that succeeded (as the desktop's
 * `LanguageCard`: switching first made `I18nProvider` flip back to the stored language until the
 * refetch), then closes the sheet. A failed save leaves it open with the toast.
 */
export function LanguagePickerSheet({ open, onOpenChange }: LanguagePickerSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const { language, setLanguage } = useLanguage();
  const { updateProfile } = useSettingsMutations();

  const select = (code: SupportedLanguage) => {
    if (updateProfile.isPending) return;
    if (code === language) {
      onOpenChange(false);
      return;
    }
    updateProfile.mutate(
      { language: code },
      {
        onSuccess: () => {
          setLanguage(code);
          onOpenChange(false);
        },
      }
    );
  };

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('settings.language.title')}
      description={t('settings.language.lead')}
      full
      footer={
        <Button
          variant="ghost"
          className="h-12 flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.cancel')}
        </Button>
      }
    >
      <div
        role="radiogroup"
        aria-label={t('settings.language.title')}
        aria-busy={updateProfile.isPending || undefined}
        className="divide-y divide-line-soft"
      >
        {LANGUAGES.map((info) => (
          <PickRow
            key={info.code}
            label={info.native}
            checked={info.code === language}
            onSelect={() => select(info.code)}
          />
        ))}
      </div>
    </Sheet>
  );
}
