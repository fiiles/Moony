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
   * `field`: an outlined control for a settings row.
   */
  variant?: 'quiet' | 'field';
  /** Shows each language's format with this currency; without it, a plain number. */
  currency?: string;
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
  disabled,
  className,
}: LanguageSelectProps) {
  const { t } = useTranslation('auth');
  const current = languageInfo(value);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild disabled={disabled}>
        <Button
          type="button"
          variant={variant === 'field' ? 'outline' : 'ghost'}
          size={variant === 'field' ? 'default' : 'sm'}
          aria-label={`${t('language.select')}: ${current.native}`}
          className={cn(variant === 'field' && 'w-[200px] justify-between font-550', className)}
        >
          {variant === 'quiet' && <Globe aria-hidden />}
          <span lang={current.code} className="truncate">
            {current.native}
          </span>
          <ChevronDown aria-hidden className={cn(variant === 'field' && 'ml-auto')} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[230px]">
        <DropdownMenuRadioGroup
          value={value}
          onValueChange={(code) => {
            if (isSupportedLanguage(code) && code !== value) onChange(code);
          }}
        >
          {LANGUAGES.map((language) => (
            <DropdownMenuRadioItem key={language.code} value={language.code}>
              <span lang={language.code}>{language.native}</span>
              <span className="ml-auto pl-4 font-500 text-ink-4 num">
                {formatSample(language.locale, currency)}
              </span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
