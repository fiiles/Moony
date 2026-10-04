import { useTranslation } from 'react-i18next';
import { LANGUAGE_NAMES, SUPPORTED_LANGUAGES, type SupportedLanguage } from '@/i18n/index';
import { Segmented } from '@/components/ui/segmented';

interface LanguageSegmentProps {
  language: SupportedLanguage;
  onLanguageChange: (lang: SupportedLanguage) => void;
  className?: string;
}

/** Language switch of the lock screen and the first run (prototype lock.html): a segment, not a select. */
export function LanguageSegment({ language, onLanguageChange, className }: LanguageSegmentProps) {
  const { t } = useTranslation('auth');
  return (
    <Segmented
      className={className}
      aria-label={t('language.select')}
      value={language}
      onValueChange={onLanguageChange}
      options={SUPPORTED_LANGUAGES.map((lang) => ({
        value: lang,
        label: LANGUAGE_NAMES[lang].native,
      }))}
    />
  );
}
