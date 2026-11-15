import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { CURRENCY_CODES, currencyName, type CurrencyCode } from '@shared/currencies';
import { currencyMatchScore } from '@/lib/currency-search';
import { useFormat } from '@/lib/use-format';
import { PickGroup, PickRow } from '@/mobile/components/PickRow';
import { SearchField } from '@/mobile/components/SearchField';
import { Sheet } from '@/mobile/components/Sheet';
import { usePendingPick } from '@/mobile/hooks/use-pending-pick';
import { frequentCurrencies } from '@/mobile/lib/currency-pick';

interface CurrencyPickerSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The chosen currency, checked in the list. */
  value: CurrencyCode;
  /**
   * The pick. A promise (the main currency's save) keeps the sheet open on the picked row, spinning,
   * until it settles: it closes once the save succeeded and stays for another pick after a failure.
   * Anything else closes the sheet at once.
   */
  onPick: (code: CurrencyCode) => void | Promise<unknown>;
  /** Restricts the list (the main currency needs a rate source); every supported currency by default. */
  options?: readonly CurrencyCode[];
  /** What is being chosen; "Hlavní měna" by default (the first account's currency says "Měna"). */
  title?: string;
  /** What to say above the search (Nastavení's hints on the main currency); nothing by default. */
  lead?: ReactNode;
}

/**
 * A currency picker (first run): the search over the list, one 44 pt row per currency as
 * "CZK · Česká koruna" with a check on the chosen one. The search matches the code and the name
 * in the UI language and in English, ignoring case and accents (the desktop combobox's ranking).
 * A tap picks the currency and closes the sheet. A `lead` scrolls with the list, above the search.
 * With the search empty, "Časté" (the chosen currency, the euro and the dollar) comes first and
 * "Všechny měny" holds the rest (audit A10). A pick that saves (`onPick` returns a promise) shows on
 * its row while it runs, the other rows faded (audit E3).
 */
export function CurrencyPickerSheet({
  open,
  onOpenChange,
  value,
  onPick,
  options = CURRENCY_CODES,
  title,
  lead,
}: CurrencyPickerSheetProps) {
  const { t } = useTranslation('mobile');
  const heading = title ?? t('firstRun.currencyPick');
  const { pending, run } = usePendingPick<CurrencyCode>(t('settings.saveTimeout'));

  return (
    <Sheet open={open} onOpenChange={onOpenChange} title={heading} full dismiss="cancel">
      {/* Its own component: the search starts empty every time the sheet opens */}
      <CurrencyList
        label={heading}
        lead={lead}
        value={value}
        options={options}
        pending={pending}
        onPick={(code) => {
          const saving = onPick(code);
          if (saving instanceof Promise)
            run(
              code,
              () => saving,
              () => onOpenChange(false)
            );
          else onOpenChange(false);
        }}
      />
    </Sheet>
  );
}

function CurrencyList({
  label,
  lead,
  value,
  options,
  pending,
  onPick,
}: {
  label: string;
  lead: ReactNode;
  value: CurrencyCode;
  options: readonly CurrencyCode[];
  pending: CurrencyCode | null;
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

  const searching = query.trim() !== '';
  const frequentCodes = frequentCurrencies(value, options);
  const frequent = frequentCodes.flatMap((code) => rows.find((row) => row.code === code) ?? []);
  const pickRow = (row: (typeof rows)[number]) => (
    <PickRow
      key={row.code}
      label={row.name === row.code ? row.code : `${row.code} · ${row.name}`}
      checked={row.code === value}
      pending={pending === row.code}
      dimmed={pending !== null && pending !== row.code}
      onSelect={() => onPick(row.code)}
    />
  );

  const shown = !searching
    ? rows
    : rows
        .map((row) => ({ row, score: currencyMatchScore(row.code, row.names, query) }))
        .filter(({ score }) => score > 0)
        // Best match first; the sort is stable, so equal scores keep the code order
        .sort((a, b) => b.score - a.score)
        .map(({ row }) => row);

  return (
    <>
      {lead}
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
      ) : searching ? (
        <div role="radiogroup" aria-label={label} className="divide-y divide-line-soft">
          {shown.map(pickRow)}
        </div>
      ) : (
        <>
          <PickGroup title={t('currencyPicker.frequent')}>{frequent.map(pickRow)}</PickGroup>
          <PickGroup title={t('currencyPicker.all')}>
            {shown.filter((row) => !frequentCodes.includes(row.code)).map(pickRow)}
          </PickGroup>
        </>
      )}
    </>
  );
}
