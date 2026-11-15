import { useTranslation } from 'react-i18next';
import { useSettingsMutations } from '@/hooks/use-settings-mutations';
import { useLanguage } from '@/i18n/I18nProvider';
import { LANGUAGES, type SupportedLanguage } from '@/i18n/languages';
import { PickRow } from '@/mobile/components/PickRow';
import { Sheet } from '@/mobile/components/Sheet';
import { usePendingPick } from '@/mobile/hooks/use-pending-pick';

interface LanguagePickerSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * "Jazyk aplikace" (spec §8.9): one row per language in its own name, the current one checked.
 * A tap saves the profile first and switches the UI once that succeeded (as the desktop's
 * `LanguageCard`: switching first made `I18nProvider` flip back to the stored language until the
 * refetch), then closes the sheet. While it saves, the picked row spins and the others wait,
 * faded (audit E3); a failed save leaves the sheet open with the toast, a save that hangs says so
 * after a while and lets the person pick again.
 */
export function LanguagePickerSheet({ open, onOpenChange }: LanguagePickerSheetProps) {
  const { t } = useTranslation('mobile');
  const { language, setLanguage } = useLanguage();
  const { updateProfile } = useSettingsMutations();

  const { pending, run } = usePendingPick<SupportedLanguage>(t('settings.saveTimeout'));

  const select = (code: SupportedLanguage) => {
    if (code === language) {
      onOpenChange(false);
      return;
    }
    run(
      code,
      () => updateProfile.mutateAsync({ language: code }).then(() => setLanguage(code)),
      () => onOpenChange(false)
    );
  };

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('settings.language.title')}
      description={t('settings.language.lead')}
      full
      dismiss="cancel"
    >
      <div
        role="radiogroup"
        aria-label={t('settings.language.title')}
        aria-busy={pending !== null || undefined}
        className="divide-y divide-line-soft"
      >
        {LANGUAGES.map((info) => (
          <PickRow
            key={info.code}
            label={info.native}
            lang={info.code}
            checked={info.code === language}
            pending={pending === info.code}
            dimmed={pending !== null && pending !== info.code}
            onSelect={() => select(info.code)}
          />
        ))}
      </div>
    </Sheet>
  );
}
