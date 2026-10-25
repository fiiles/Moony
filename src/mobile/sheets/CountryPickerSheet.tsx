import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { RULE_PACK_COUNTRY_IDS, countryOptionsFromPacks } from '@/utils/locale-defaults';
import { PickRow } from '@/mobile/components/PickRow';
import { Sheet } from '@/mobile/components/Sheet';

interface CountryPickerSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The chosen country's rule-pack id ("cz"), or `null` for "Jiná / není v seznamu". */
  value: string | null;
  onPick: (country: string | null) => void;
}

/**
 * "Země bankovnictví" (first run): "Jiná / není v seznamu" first, then the countries that have a
 * rule pack, named in the UI language and sorted by that name, the chosen one checked. The country
 * decides which categorization rules are switched on and whose banks the import offers first. A
 * tap picks it and closes the sheet.
 */
export function CountryPickerSheet({ open, onOpenChange, value, onPick }: CountryPickerSheetProps) {
  const { t, i18n } = useTranslation('mobile');
  const { t: ta } = useTranslation('auth');
  const { t: tc } = useTranslation('common');
  const options = useMemo(
    () => countryOptionsFromPacks([...RULE_PACK_COUNTRY_IDS], i18n.language),
    [i18n.language]
  );

  const pick = (country: string | null) => {
    onPick(country);
    onOpenChange(false);
  };

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('firstRun.countryPick')}
      full
      footer={
        <Button
          variant="ghost"
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          onClick={() => onOpenChange(false)}
        >
          {tc('buttons.cancel')}
        </Button>
      }
    >
      <div
        role="radiogroup"
        aria-label={t('firstRun.countryPick')}
        className="divide-y divide-line-soft"
      >
        <PickRow
          label={ta('onboarding.welcome.countryNone')}
          checked={value === null}
          onSelect={() => pick(null)}
        />
        {options.map((option) => (
          <PickRow
            key={option.packId}
            label={option.name}
            checked={option.packId === value}
            onSelect={() => pick(option.packId)}
          />
        ))}
      </div>
    </Sheet>
  );
}
