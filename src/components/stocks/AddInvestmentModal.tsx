import { useEffect, useState } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Loader2, Search, Upload } from 'lucide-react';
import { requiredNumber } from '@/utils/form-schemas';
import { investmentsApi, priceApi, type StockSearchResult } from '@/lib/tauri-api';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { exchangeName, resolveTickerCurrency } from '@/utils/exchange-names';
import { currencyCodeSchema, type CurrencyCode } from '@shared/currencies';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input, InputWrap } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';

const formSchema = z.object({
  companyName: z.string().min(1, 'validation.companyNameRequired'),
  ticker: z.string().min(1, 'validation.tickerRequired'),
  quantity: requiredNumber('validation.quantityRequired').positive('validation.quantityPositive'),
  pricePerUnit: requiredNumber('validation.priceRequired').positive('validation.pricePositive'),
  currency: currencyCodeSchema,
  date: z.string().optional(), // Input type="date" returns string
});

/** Pause after the last keystroke before the ticker search starts. */
const SEARCH_DEBOUNCE_MS = 400;
/** Typing shorter than this does not search; the search button still does. */
const MIN_AUTO_SEARCH_CHARS = 2;
/** The search result is cached for the session so retyping a name is instant. */
const SEARCH_STALE_MS = 5 * 60 * 1000;

interface AddInvestmentModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Opens the CSV import wizard from the footer ("Importovat CSV"). */
  onImportCsv?: () => void;
  /** Prefill when buying a watched stock ("Koupit do portfolia"). */
  initial?: { ticker: string; companyName?: string | null; currency?: string | null };
}

/**
 * "Přidat investici" (design system §6 Modal, prototype stocks.html): ticker
 * search against Yahoo Finance fills the name, then date + quantity and
 * price + currency in paired rows; the footer offers the CSV import as the
 * secondary way in.
 */
export function AddInvestmentModal({
  open,
  onOpenChange,
  onImportCsv,
  initial,
}: AddInvestmentModalProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      companyName: '',
      ticker: '',
      quantity: 0,
      pricePerUnit: 0,
      currency: 'USD',
      date: new Date().toISOString().split('T')[0],
    },
  });

  // A watched stock arrives with its ticker, name and currency already known
  useEffect(() => {
    if (!open || !initial) return;
    form.reset({
      companyName: initial.companyName ?? '',
      ticker: initial.ticker,
      quantity: 0,
      pricePerUnit: 0,
      currency: (initial.currency as CurrencyCode | null) ?? 'USD',
      date: new Date().toISOString().split('T')[0],
    });
  }, [open, initial, form]);

  // the ticker search runs once typing pauses and shows its results inline
  const [typedQuery, setTypedQuery] = useState('');
  const debouncedQuery = useDebouncedValue(typedQuery, SEARCH_DEBOUNCE_MS);
  // The search button skips the debounce and the minimum length
  const [forcedQuery, setForcedQuery] = useState<string | null>(null);
  const [suggestionsOpen, setSuggestionsOpen] = useState(false);
  const queryText = (forcedQuery ?? typedQuery).trim();
  const minChars = forcedQuery === null ? MIN_AUTO_SEARCH_CHARS : 1;
  const showSuggestions = open && suggestionsOpen && queryText.length >= minChars;
  const typingPending = forcedQuery === null && debouncedQuery !== typedQuery;

  const {
    data: searchResults = [],
    isFetching: searching,
    isError: searchFailed,
  } = useQuery<StockSearchResult[]>({
    queryKey: ['stock-ticker-search', queryText],
    queryFn: () => priceApi.searchStockTickers(queryText),
    enabled: showSuggestions && !typingPending,
    staleTime: SEARCH_STALE_MS,
  });
  const waiting = typingPending || searching;

  const handleSearchTyping = (value: string) => {
    setTypedQuery(value);
    setForcedQuery(null);
    setSuggestionsOpen(true);
  };
  const searchNow = (value: string) => {
    const query = value.trim();
    if (!query) return;
    setForcedQuery(query);
    setSuggestionsOpen(true);
  };
  const closeSuggestions = () => {
    setSuggestionsOpen(false);
    setForcedQuery(null);
    setTypedQuery('');
  };

  const selectTicker = (result: StockSearchResult) => {
    form.setValue('ticker', result.symbol, { shouldValidate: true });
    form.setValue('companyName', result.shortname || result.symbol, { shouldValidate: true });
    // the quote's currency, or the exchange's usual one; still editable below
    form.setValue('currency', resolveTickerCurrency(result.exchange).currency, {
      shouldValidate: true,
    });
    setSuggestionsOpen(false);
  };

  const createInvestment = useMutation({
    mutationFn: async (values: z.infer<typeof formSchema>) => {
      const investmentData = { ticker: values.ticker, companyName: values.companyName };
      const initialTransaction = {
        type: 'buy',
        ticker: values.ticker,
        companyName: values.companyName,
        quantity: values.quantity.toString(),
        pricePerUnit: values.pricePerUnit.toString(),
        currency: values.currency,
        transactionDate: values.date
          ? Math.floor(new Date(values.date).getTime() / 1000)
          : Math.floor(Date.now() / 1000),
      };
      return investmentsApi.create(investmentData, initialTransaction);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['all-stock-transactions'] });
      onOpenChange(false);
      form.reset();
      toast(t('toast.added'));

      // Prices and dividends for the new stock refresh in the background
      priceApi
        .refreshStockPrices()
        .then(() => {
          queryClient.invalidateQueries({ queryKey: ['investments'] });
          queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
        })
        .catch(console.error);
      priceApi
        .refreshDividends()
        .then(() => {
          queryClient.invalidateQueries({ queryKey: ['investments'] });
          queryClient.invalidateQueries({ queryKey: ['dividend-summary'] });
        })
        .catch(console.error);
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  const currency = form.watch('currency');

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) closeSuggestions();
        onOpenChange(next);
      }}
    >
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t('modal.add.title')}</DialogTitle>
          <DialogDescription>{t('modal.add.description')}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id="add-investment-form"
            onSubmit={form.handleSubmit((values) => createInvestment.mutate(values))}
            className="space-y-4"
          >
            <div className="grid grid-cols-[1fr_1.4fr] gap-3">
              <FormField
                control={form.control}
                name="ticker"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.ticker')}</FormLabel>
                    <div className="flex gap-2">
                      <FormControl>
                        <Input
                          placeholder="AAPL"
                          autoFocus
                          autoComplete="off"
                          {...field}
                          onChange={(e) => {
                            field.onChange(e);
                            handleSearchTyping(e.target.value);
                          }}
                        />
                      </FormControl>
                      <Button
                        type="button"
                        variant="outline"
                        size="icon"
                        className="size-input shrink-0"
                        onClick={() => searchNow(field.value || form.getValues('companyName'))}
                        disabled={!field.value && !form.getValues('companyName')}
                        aria-label={t('modal.add.search')}
                        title={t('modal.add.search')}
                      >
                        {waiting && showSuggestions ? (
                          <Loader2 className="animate-spin" />
                        ) : (
                          <Search />
                        )}
                      </Button>
                    </div>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="companyName"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.companyName')}</FormLabel>
                    <FormControl>
                      <Input
                        placeholder="Apple Inc."
                        autoComplete="off"
                        {...field}
                        onChange={(e) => {
                          field.onChange(e);
                          handleSearchTyping(e.target.value);
                        }}
                      />
                    </FormControl>
                    {!showSuggestions && (
                      <FormDescription>{t('modal.add.searchHint')}</FormDescription>
                    )}
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>

            {showSuggestions && (
              <div
                className="overflow-hidden rounded-r3 border border-line-strong bg-paper shadow-pop"
                aria-live="polite"
              >
                {waiting && searchResults.length === 0 ? (
                  <p className="flex items-center gap-2 px-3 py-2.5 text-table text-ink-3">
                    <Loader2 className="size-3.5 animate-spin" />
                    {t('modal.add.searching')}
                  </p>
                ) : searchFailed ? (
                  <p className="px-3 py-2.5 text-table text-loss">{t('modal.add.searchError')}</p>
                ) : searchResults.length === 0 ? (
                  <p className="px-3 py-2.5 text-table text-ink-3">
                    {t('modal.add.noTickerFound')}
                  </p>
                ) : (
                  <ul className="m-0 max-h-56 list-none overflow-y-auto p-1">
                    {searchResults.map((result) => (
                      <li key={`${result.symbol}-${result.exchange}`}>
                        <button
                          type="button"
                          onClick={() => selectTicker(result)}
                          className="flex w-full items-baseline gap-2.5 rounded-r1 px-2.5 py-2 text-left text-table text-ink transition-colors duration-fast hover:bg-well focus-visible:bg-well focus-visible:outline-none"
                        >
                          <b className="w-16 shrink-0 font-650 num">{result.symbol}</b>
                          <span className="min-w-0 flex-1 truncate text-ink-2">
                            {result.shortname}
                          </span>
                          <span className="shrink-0 text-micro font-500 text-ink-4">
                            {exchangeName(result.exchange)}
                          </span>
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}

            <div className="grid grid-cols-2 gap-3">
              <FormField
                control={form.control}
                name="date"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.purchaseDate')}</FormLabel>
                    <FormControl>
                      <Input type="date" {...field} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="quantity"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.quantity')}</FormLabel>
                    <InputWrap unit={t('table.unit')}>
                      <FormControl>
                        <Input type="number" step="0.0001" placeholder="0" {...field} />
                      </FormControl>
                    </InputWrap>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>

            <div className="grid grid-cols-2 gap-3">
              <FormField
                control={form.control}
                name="pricePerUnit"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.pricePerShare')}</FormLabel>
                    <InputWrap unit={currency}>
                      <FormControl>
                        <Input
                          type="number"
                          step="0.01"
                          placeholder="0.00"
                          {...field}
                          onBlur={(e) => {
                            const value = parseFloat(e.target.value);
                            if (!isNaN(value)) field.onChange(value.toFixed(2));
                            field.onBlur();
                          }}
                        />
                      </FormControl>
                    </InputWrap>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="currency"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{tc('labels.currency')}</FormLabel>
                    <FormControl>
                      <CurrencyCombobox value={field.value} onChange={field.onChange} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>
          </form>
        </Form>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          {onImportCsv && (
            <Button
              type="button"
              variant="outline"
              onClick={() => {
                onOpenChange(false);
                onImportCsv();
              }}
            >
              <Upload />
              {t('importCSV')}
            </Button>
          )}
          <Button
            type="submit"
            form="add-investment-form"
            disabled={createInvestment.isPending}
            loading={createInvestment.isPending}
          >
            {createInvestment.isPending ? tc('status.adding') : t('addInvestment')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
