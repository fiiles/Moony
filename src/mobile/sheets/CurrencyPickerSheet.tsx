import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CURRENCY_CODES, currencyName, type CurrencyCode } from '@shared/currencies';
import { Button } from '@/components/ui/button';
import { currencyMatchScore } from '@/lib/currency-search';
import { useFormat } from '@/lib/use-format';
import { PickRow } from '@/mobile/components/PickRow';
import { SearchField } from '@/mobile/components/SearchField';
import { Sheet } from '@/mobile/components/Sheet';

interface CurrencyPickerSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The chosen currency, checked in the list. */
  value: CurrencyCode;
  onPick: (code: CurrencyCode) => void;
  /** Restricts the list (the main currency needs a rate source); every supported currency by default. */
  options?: readonly CurrencyCode[];
  /** What is being chosen; "Hlavní měna" by default (the first account's currency says "Měna"). */
  title?: string;
}

/**
 * A currency picker (first run): the search over the list, one 44 pt row per currency as
 * "CZK · Česká koruna" with a check on the chosen one. The search matches the code and the name
 * in the UI language and in English, ignoring case and accents (the desktop combobox's ranking).
 * A tap picks the currency and closes the sheet.
 */
export function CurrencyPickerSheet({
  open,
  onOpenChange,
  value,
  onPick,
  options = CURRENCY_CODES,
  title,
}: CurrencyPickerSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const heading = title ?? t('firstRun.currencyPick');

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={heading}
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
      {/* Its own component: the search starts empty every time the sheet opens */}
      <CurrencyList
        label={heading}
        value={value}
        options={options}
        onPick={(code) => {
          onPick(code);
          onOpenChange(false);
        }}
      />
    </Sheet>
  );
}

function CurrencyList({
  label,
  value,
  options,
  onPick,
}: {
  label: string;
  value: CurrencyCode;
  options: readonly CurrencyCode[];
  onPick: (code: CurrencyCode) => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const [query, setQuery] = useState('');

  const rows = useMemo(
    () =>
      options.map((code) => {
        const name = currencyName(code, fmt.locale);
        const english = currencyName(code, 'en');
        return {
          code,
          name,
          // "dollar" finds the dollar in a Czech UI too
          names: name === english ? [name] : [name, english],
        };
      }),
    [options, fmt.locale]
  );

  const shown =
    query.trim() === ''
      ? rows
      : rows
          .map((row) => ({ row, score: currencyMatchScore(row.code, row.names, query) }))
          .filter(({ score }) => score > 0)
          // Best match first; the sort is stable, so equal scores keep the code order
          .sort((a, b) => b.score - a.score)
          .map(({ row }) => row);

  return (
    <>
      <SearchField
        value={query}
        onChange={setQuery}
        placeholder={t('firstRun.searchCurrency')}
        clearLabel={tc('a11y.clearSearch')}
      />
      {shown.length === 0 ? (
        <p role="status" className="m-0 px-1 py-4 text-m-sub text-ink-3">
          {tc('labels.noCurrencyFound')}
        </p>
      ) : (
        <div role="radiogroup" aria-label={label} className="divide-y divide-line-soft">
          {shown.map((row) => (
            <PickRow
              key={row.code}
              label={row.name === row.code ? row.code : `${row.code} · ${row.name}`}
              checked={row.code === value}
              onSelect={() => onPick(row.code)}
            />
          ))}
        </div>
      )}
    </>
  );
}
