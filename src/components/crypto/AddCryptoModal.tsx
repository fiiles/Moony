import { useState } from 'react';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Loader2, Search } from 'lucide-react';
import { requiredNumber } from '@/utils/form-schemas';
import { cryptoApi, priceApi, type CryptoSearchResult } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { toPlainDecimal } from '@/utils/plain-decimal';
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
import { CryptoSearchList } from '@/components/crypto/CryptoSearchList';
import { useCryptoSearch } from '@/hooks/use-crypto-search';

const formSchema = z.object({
  name: z.string().min(1, 'validation.nameRequired'),
  ticker: z.string().min(1, 'validation.tickerRequired'),
  yahooSymbol: z.string().optional(),
  quantity: requiredNumber('validation.quantityRequired').positive('validation.quantityPositive'),
  pricePerUnit: requiredNumber('validation.priceRequired').min(0, 'validation.priceNonNegative'),
  currency: currencyCodeSchema,
  date: z.string().optional(),
});

interface AddCryptoModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * "Přidat kryptoměnu" (design system §6 Modal, prototype crypto.html): typing
 * a coin name shows Yahoo Finance suggestions inline (name and ticker); picking
 * one fills the ticker and the Yahoo symbol (editing the ticker by hand drops the symbol),
 * then quantity, rate and date. The first purchase becomes the position's first transaction.
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
      yahooSymbol: '',
      quantity: 0,
      pricePerUnit: 0,
      currency: 'USD',
      date: new Date().toISOString().split('T')[0],
    },
  });

  const [typedQuery, setTypedQuery] = useState('');
  const [forcedQuery, setForcedQuery] = useState<string | null>(null);
  const [suggestionsOpen, setSuggestionsOpen] = useState(false);
  const search = useCryptoSearch({
    typed: typedQuery,
    forced: forcedQuery,
    enabled: open && suggestionsOpen,
  });

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

  const selectCoin = (result: CryptoSearchResult) => {
    form.setValue('ticker', result.ticker, { shouldValidate: true });
    form.setValue('name', result.name, { shouldValidate: true });
    form.setValue('yahooSymbol', result.symbol);
    setSuggestionsOpen(false);
  };

  const createInvestment = useMutation({
    mutationFn: async (values: z.infer<typeof formSchema>) => {
      const cryptoData = {
        ticker: values.ticker,
        name: values.name,
        yahooSymbol: values.yahooSymbol || undefined,
      };
      const initialTransaction = {
        type: 'buy',
        ticker: values.ticker,
        name: values.name,
        quantity: toPlainDecimal(values.quantity),
        pricePerUnit: toPlainDecimal(values.pricePerUnit),
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
        .then(() => invalidateAfterPriceRefresh(queryClient))
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
  // The main-currency estimate is left out when no rate is known (never 1:1).
  const totalInMain = currency === currencyCode ? null : convert(total, currency);
  const totalDisplay =
    totalInMain === null
      ? fmt.money(total, currency)
      : `${fmt.money(total, currency)} ≈ ${fmt.money(totalInMain, currencyCode)}`;

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
                        {search.waiting && search.show ? (
                          <Loader2 className="animate-spin" />
                        ) : (
                          <Search />
                        )}
                      </Button>
                    </div>
                    {!search.show && <FormDescription>{t('modal.add.searchHint')}</FormDescription>}
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
                        onChange={(e) => {
                          field.onChange(e);
                          // A hand-edited ticker is no longer the picked listing's
                          form.setValue('yahooSymbol', '');
                        }}
                      />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>

            {search.show && <CryptoSearchList search={search} onPick={selectCoin} />}

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
                        <Input type="number" step="any" placeholder="0.00" {...field} />
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
