import { forwardRef, useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronsUpDown } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command';
import { cn } from '@/lib/utils';
import { useFormat } from '@/lib/use-format';
import { currencyMatchScore } from '@/lib/currency-search';
import { CURRENCY_CODES, currencyName, type CurrencyCode } from '@shared/currencies';

export interface CurrencyComboboxProps extends Omit<
  React.ComponentPropsWithoutRef<typeof Button>,
  'value' | 'onChange' | 'children' | 'variant' | 'type'
> {
  value: string | null | undefined;
  onChange: (code: CurrencyCode) => void;
  /** Restrict the list (a form that supports only a subset); defaults to every supported currency. */
  options?: readonly CurrencyCode[];
  placeholder?: string;
  /** Show the localized name next to the code in the trigger (turn off in narrow fields). */
  showName?: boolean;
}

/**
 * Searchable currency picker: code, localized name and an example amount per
 * row; type to filter by code or name. Drop-in replacement for a currency
 * `<Select>`, including inside react-hook-form's `<FormControl>` (the trigger
 * receives its id/aria props through `...rest`).
 */
export const CurrencyCombobox = forwardRef<HTMLButtonElement, CurrencyComboboxProps>(
  function CurrencyCombobox(
    {
      value,
      onChange,
      options = CURRENCY_CODES,
      placeholder,
      showName = true,
      className,
      disabled,
      ...rest
    },
    ref
  ) {
    const { t } = useTranslation('common');
    const fmt = useFormat();
    const [open, setOpen] = useState(false);
    const [highlighted, setHighlighted] = useState<string>('');
    const listRef = useRef<HTMLDivElement>(null);

    const rows = useMemo(
      () =>
        options.map((code) => {
          const name = currencyName(code, fmt.locale);
          const englishName = currencyName(code, 'en');
          return {
            code,
            name,
            // Search by the English name too, so "dollar" works in a Czech UI.
            names: name === englishName ? [name] : [name, englishName],
            example: fmt.money(1234.56, code),
          };
        }),
      [options, fmt]
    );

    const selected = rows.find((row) => row.code === value);
    const selectedName = selected?.name ?? (value ? currencyName(value, fmt.locale) : '');

    // Open on the current currency: highlight it and bring it into view.
    useEffect(() => {
      if (!open) return;
      setHighlighted(value ?? '');
      const timer = setTimeout(() => {
        listRef.current
          ?.querySelector('[cmdk-item][data-selected="true"]')
          ?.scrollIntoView({ block: 'nearest' });
      }, 0);
      return () => clearTimeout(timer);
    }, [open, value]);

    return (
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            ref={ref}
            type="button"
            variant="outline"
            role="combobox"
            aria-expanded={open}
            aria-haspopup="listbox"
            disabled={disabled}
            className={cn(
              'h-9 w-full justify-between bg-transparent px-3 font-normal',
              !value && 'text-ink-3',
              className
            )}
            {...rest}
          >
            <span className="truncate">
              {value ? (
                <>
                  <span className="font-medium">{value}</span>
                  {showName && selectedName && selectedName !== value && (
                    <span className="text-ink-3"> · {selectedName}</span>
                  )}
                </>
              ) : (
                (placeholder ?? t('labels.selectCurrency'))
              )}
            </span>
            <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
          </Button>
        </PopoverTrigger>
        <PopoverContent
          className="w-[--radix-popover-trigger-width] min-w-[18rem] p-0"
          align="start"
        >
          {/* Keeps the wheel scrolling the list when the popover sits inside a Dialog. */}
          <div onWheel={(e) => e.stopPropagation()}>
            <Command
              label={t('labels.searchCurrency')}
              value={highlighted}
              onValueChange={setHighlighted}
              filter={(itemValue, search, keywords) =>
                currencyMatchScore(itemValue, keywords ?? [], search)
              }
            >
              <CommandInput placeholder={t('labels.searchCurrency')} />
              <CommandList ref={listRef} className="max-h-[260px]">
                <CommandEmpty>{t('labels.noCurrencyFound')}</CommandEmpty>
                <CommandGroup>
                  {rows.map((row) => (
                    <CommandItem
                      key={row.code}
                      value={row.code}
                      keywords={row.names}
                      onSelect={() => {
                        onChange(row.code);
                        setOpen(false);
                      }}
                    >
                      <Check
                        className={cn('h-4 w-4', row.code === value ? 'opacity-100' : 'opacity-0')}
                      />
                      <span className="w-9 shrink-0 font-medium">{row.code}</span>
                      <span className="min-w-0 flex-1 truncate">{row.name}</span>
                      <span className="shrink-0 text-xs tabular-nums text-ink-3">
                        {row.example}
                      </span>
                    </CommandItem>
                  ))}
                </CommandGroup>
              </CommandList>
            </Command>
          </div>
        </PopoverContent>
      </Popover>
    );
  }
);
