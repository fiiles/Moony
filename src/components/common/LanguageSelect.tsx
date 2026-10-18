import { ChevronDown, Globe } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  LANGUAGES,
  isSupportedLanguage,
  languageInfo,
  type SupportedLanguage,
} from '@/i18n/languages';
import { getFormatters } from '@/lib/format';
import { cn } from '@/lib/utils';

interface LanguageSelectProps {
  value: SupportedLanguage;
  onChange: (language: SupportedLanguage) => void;
  /**
   * `quiet`: a text button with a globe (lock screen, first run).
   * `field`: the standard select field for a settings row.
   */
  variant?: 'quiet' | 'field';
  /** Shows each language's format with this currency; without it, a plain number. */
  currency?: string;
  /** First-run setup only needs language names; settings retain format examples. */
  showFormatSample?: boolean;
  disabled?: boolean;
  className?: string;
}

/** The sample of a language's format next to its name: "1.234,56 €" for German. */
function formatSample(locale: string, currency: string | undefined): string {
  const fmt = getFormatters(locale, currency);
  return currency
    ? fmt.money(1234.56, currency)
    : fmt.number(1234.56, { minimumFractionDigits: 2 });
}

/**
 * The UI language picker: one menu of every shipped language, each in its own
 * name and with a sample of its number format. Replaces the two-language segment.
 */
export function LanguageSelect({
  value,
  onChange,
  variant = 'quiet',
  currency,
  showFormatSample = true,
  disabled,
  className,
}: LanguageSelectProps) {
  const { t } = useTranslation('auth');
  const current = languageInfo(value);

  const pick = (code: string) => {
    if (isSupportedLanguage(code) && code !== value) onChange(code);
  };

  const sample = (locale: string) =>
    showFormatSample && (
      <span className="ml-auto pl-4 font-500 text-ink-4 num">{formatSample(locale, currency)}</span>
    );

  if (variant === 'field') {
    return (
      <Select value={value} onValueChange={pick} disabled={disabled}>
        <SelectTrigger
          aria-label={`${t('language.select')}: ${current.native}`}
          className={cn('w-[200px]', className)}
        >
          {/* The trigger shows the name only; the format sample stays in the list. */}
          <SelectValue>
            <span lang={current.code}>{current.native}</span>
          </SelectValue>
        </SelectTrigger>
        <SelectContent align="end" className={showFormatSample ? 'min-w-[230px]' : 'min-w-[160px]'}>
          {LANGUAGES.map((language) => (
            <SelectItem
              key={language.code}
              value={language.code}
              textValue={language.native}
              className="[&>span:last-child]:flex [&>span:last-child]:w-full"
            >
              <span lang={language.code}>{language.native}</span>
              {sample(language.locale)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild disabled={disabled}>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={`${t('language.select')}: ${current.native}`}
          className={className}
        >
          <Globe aria-hidden />
          <span lang={current.code} className="truncate">
            {current.native}
          </span>
          <ChevronDown aria-hidden />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        className={showFormatSample ? 'min-w-[230px]' : 'min-w-[160px]'}
      >
        <DropdownMenuRadioGroup value={value} onValueChange={pick}>
          {LANGUAGES.map((language) => (
            <DropdownMenuRadioItem key={language.code} value={language.code}>
              <span lang={language.code}>{language.native}</span>
              {sample(language.locale)}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
