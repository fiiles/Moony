import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Loader2, Search } from 'lucide-react';
import { requiredNumber } from '@/utils/form-schemas';
import { cryptoApi, priceApi, type CoinGeckoSearchResult } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
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
import { AssetLogo } from '@/components/common/AssetLogo';

const formSchema = z.object({
  name: z.string().min(1, 'validation.nameRequired'),
  ticker: z.string().min(1, 'validation.tickerRequired'),
  coingeckoId: z.string().optional(),
  quantity: requiredNumber('validation.quantityRequired').positive('validation.quantityPositive'),
  pricePerUnit: requiredNumber('validation.priceRequired').min(0, 'validation.priceNonNegative'),
  currency: currencyCodeSchema,
  date: z.string().optional(),
});

/** Pause after the last keystroke before the CoinGecko search starts. */
const SEARCH_DEBOUNCE_MS = 400;
const MIN_AUTO_SEARCH_CHARS = 2;
const SEARCH_STALE_MS = 5 * 60 * 1000;

interface AddCryptoModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * "Přidat kryptoměnu" (design system §6 Modal, prototype crypto.html): typing
 * a coin name shows CoinGecko suggestions inline (rank and ticker); picking one
 * fills the ticker and the CoinGecko id, then quantity, rate and date. The
 * first purchase becomes the position's first transaction.
 */
export function AddCryptoModal({ open, onOpenChange }: AddCryptoModalProps) {
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { convert, currencyCode } = useCurrency();
  const fmt = useFormat();

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      name: '',
      ticker: '',
      coingeckoId: '',
      quantity: 0,
      pricePerUnit: 0,
      currency: 'USD',
      date: new Date().toISOString().split('T')[0],
    },
  });

  const [typedQuery, setTypedQuery] = useState('');
  const debouncedQuery = useDebouncedValue(typedQuery, SEARCH_DEBOUNCE_MS);
  const [forcedQuery, setForcedQuery] = useState<string | null>(null);
  const [suggestionsOpen, setSuggestionsOpen] = useState(false);
  const queryText = (forcedQuery ?? typedQuery).trim();
  const minChars = forcedQuery === null ? MIN_AUTO_SEARCH_CHARS : 1;
  const showSuggestions = open && suggestionsOpen && queryText.length >= minChars;
  const typingPending = forcedQuery === null && debouncedQuery !== typedQuery;

  const {
    data: results = [],
    isFetching: searching,
    isError: searchFailed,
  } = useQuery<CoinGeckoSearchResult[]>({
    queryKey: ['crypto-search', queryText],
    queryFn: () => priceApi.searchCrypto(queryText),
    enabled: showSuggestions && !typingPending,
    staleTime: SEARCH_STALE_MS,
  });
  const waiting = typingPending || searching;

  const handleTyping = (value: string) => {
    setTypedQuery(value);
    setForcedQuery(null);
    setSuggestionsOpen(true);
  };
  const searchNow = () => {
    const query = form.getValues('name').trim();
    if (!query) return;
    setForcedQuery(query);
    setSuggestionsOpen(true);
  };
  const closeSuggestions = () => {
    setSuggestionsOpen(false);
    setForcedQuery(null);
    setTypedQuery('');
  };

  const selectCoin = (result: CoinGeckoSearchResult) => {
    form.setValue('ticker', result.symbol.toUpperCase(), { shouldValidate: true });
    form.setValue('name', result.name, { shouldValidate: true });
    form.setValue('coingeckoId', result.id);
    setSuggestionsOpen(false);
  };

  const createInvestment = useMutation({
    mutationFn: async (values: z.infer<typeof formSchema>) => {
      const cryptoData = {
        ticker: values.ticker,
        name: values.name,
        coingeckoId: values.coingeckoId || undefined,
      };
      const initialTransaction = {
        type: 'buy',
        ticker: values.ticker,
        name: values.name,
        quantity: values.quantity.toString(),
        pricePerUnit: values.pricePerUnit.toString(),
        currency: values.currency,
        transactionDate: values.date
          ? Math.floor(new Date(values.date).getTime() / 1000)
          : Math.floor(Date.now() / 1000),
      };
      return cryptoApi.create(cryptoData, initialTransaction);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['crypto'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['all-crypto-transactions'] });
      onOpenChange(false);
      form.reset();
      toast(t('toast.added'));
      // The new coin's rate arrives in the background
      priceApi
        .refreshCryptoPrices()
        .then(() => {
          queryClient.invalidateQueries({ queryKey: ['crypto'] });
          queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
        })
        .catch(console.error);
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: `${t('toast.addFailed')}: ${error.message}` });
    },
  });

  const ticker = form.watch('ticker');
  const currency = form.watch('currency') as CurrencyCode;
  const quantity = Number(form.watch('quantity')) || 0;
  const pricePerUnit = Number(form.watch('pricePerUnit')) || 0;
  const total = quantity * pricePerUnit;
  const totalDisplay =
    currency === currencyCode
      ? fmt.money(total, currency)
      : `${fmt.money(total, currency)} ≈ ${fmt.money(convert(total, currency, currencyCode as CurrencyCode), currencyCode)}`;

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
            id="add-crypto-form"
            onSubmit={form.handleSubmit((values) => createInvestment.mutate(values))}
            className="space-y-4"
          >
            <div className="grid grid-cols-[1.4fr_1fr] gap-3">
              <FormField
                control={form.control}
                name="name"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.coin')}</FormLabel>
                    <div className="flex gap-2">
                      <FormControl>
                        <Input
                          placeholder="Bitcoin"
                          autoFocus
                          autoComplete="off"
                          {...field}
                          onChange={(e) => {
                            field.onChange(e);
                            handleTyping(e.target.value);
                          }}
                        />
                      </FormControl>
                      <Button
                        type="button"
                        variant="outline"
                        size="icon"
                        className="size-input shrink-0"
                        onClick={searchNow}
                        disabled={!field.value}
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
                    {!showSuggestions && (
                      <FormDescription>{t('modal.add.searchHint')}</FormDescription>
                    )}
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="ticker"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.ticker')}</FormLabel>
                    <FormControl>
                      <Input
                        placeholder="BTC"
                        autoComplete="off"
                        className="uppercase"
                        {...field}
                      />
                    </FormControl>
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
                {waiting && results.length === 0 ? (
                  <p className="flex items-center gap-2 px-3 py-2.5 text-table text-ink-3">
                    <Loader2 className="size-3.5 animate-spin" />
                    {t('modal.add.searching')}
                  </p>
                ) : searchFailed ? (
                  <p className="px-3 py-2.5 text-table text-loss">{t('modal.add.searchError')}</p>
                ) : results.length === 0 ? (
                  <p className="px-3 py-2.5 text-table text-ink-3">{t('modal.add.noResults')}</p>
                ) : (
                  <ul className="m-0 max-h-56 list-none overflow-y-auto p-1">
                    {results.map((result) => (
                      <li key={result.id}>
                        <button
                          type="button"
                          onClick={() => selectCoin(result)}
                          className="flex w-full items-center gap-2.5 rounded-r1 px-2.5 py-2 text-left text-table text-ink transition-colors duration-fast hover:bg-well focus-visible:bg-well focus-visible:outline-none"
                        >
                          <AssetLogo ticker={result.symbol} type="crypto" variant="soft" />
                          <span className="min-w-0 flex-1 truncate">
                            <b className="font-650">{result.name}</b>{' '}
                            <span className="text-ink-4">{result.symbol.toUpperCase()}</span>
                          </span>
                          {result.market_cap_rank !== null && (
                            <span className="shrink-0 text-micro font-600 text-ink-4 num">
                              {t('modal.add.rank', { rank: result.market_cap_rank })}
                            </span>
                          )}
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
                name="quantity"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.quantity')}</FormLabel>
                    <InputWrap unit={ticker ? ticker.toUpperCase() : undefined}>
                      <FormControl>
                        <Input type="number" step="0.00000001" placeholder="0" {...field} />
                      </FormControl>
                    </InputWrap>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="pricePerUnit"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.pricePerUnit')}</FormLabel>
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
            </div>

            <div className="grid grid-cols-[1fr_138px] gap-3">
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

            <div className="flex items-center justify-between border-t border-line-soft pt-3 text-caption text-ink-3">
              <span>{t('modal.add.total')}</span>
              <b className="text-[14px] font-650 tracking-[-0.03em] text-ink num">
                {total > 0 ? totalDisplay : '—'}
              </b>
            </div>
          </form>
        </Form>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form="add-crypto-form"
            disabled={createInvestment.isPending}
            loading={createInvestment.isPending}
          >
            {createInvestment.isPending ? tc('status.adding') : t('modal.add.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
