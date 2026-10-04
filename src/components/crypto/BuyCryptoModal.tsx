import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { CryptoHoldingData } from '@shared/calculations';
import { currencyCodeSchema, type CurrencyCode } from '@shared/currencies';
import { requiredNumber } from '@/utils/form-schemas';
import { cryptoApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
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
  quantity: requiredNumber('validation.quantityRequired').positive('validation.quantityPositive'),
  pricePerUnit: requiredNumber('validation.priceRequired').min(0, 'validation.priceNonNegative'),
  currency: currencyCodeSchema,
  date: z.string().optional(),
});

interface BuyCryptoModalProps {
  investment: CryptoHoldingData | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** "Koupit" (prototype crypto-detail.html): quantity and rate in the coin's native currency, a live total. */
export function BuyCryptoModal({ investment, open, onOpenChange }: BuyCryptoModalProps) {
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { convert, currencyCode } = useCurrency();
  const fmt = useFormat();

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      quantity: 0,
      pricePerUnit: 0,
      currency: 'USD',
      date: new Date().toISOString().split('T')[0],
    },
  });

  // The native rate (usually USD) is the sensible prefill
  useEffect(() => {
    if (!investment || !open) return;
    const native = Number(investment.originalPrice ?? investment.currentPrice) || 0;
    form.reset({
      quantity: 0,
      pricePerUnit: native ? Number(native.toFixed(2)) : 0,
      currency: (investment.currency as CurrencyCode) || 'USD',
      date: new Date().toISOString().split('T')[0],
    });
  }, [investment, open, form]);

  const buyMutation = useMutation({
    mutationFn: async (values: z.infer<typeof formSchema>) => {
      if (!investment) return;
      return cryptoApi.createTransaction(investment.id, {
        type: 'buy',
        ticker: investment.ticker,
        name: investment.name,
        quantity: values.quantity.toString(),
        pricePerUnit: values.pricePerUnit.toString(),
        currency: values.currency,
        transactionDate: values.date
          ? Math.floor(new Date(values.date).getTime() / 1000)
          : Math.floor(Date.now() / 1000),
      });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['crypto'] });
      queryClient.invalidateQueries({ queryKey: ['crypto-detail', investment?.id] });
      queryClient.invalidateQueries({ queryKey: ['crypto-transactions', investment?.id] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['all-crypto-transactions'] });
      onOpenChange(false);
      toast(t('toast.bought'));
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  if (!investment) return null;

  const currency = form.watch('currency') as CurrencyCode;
  const total = (Number(form.watch('quantity')) || 0) * (Number(form.watch('pricePerUnit')) || 0);
  const totalDisplay =
    currency === currencyCode
      ? fmt.money(total, currency)
      : `${fmt.money(total, currency)} ≈ ${fmt.money(convert(total, currency, currencyCode as CurrencyCode), currencyCode)}`;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('modal.buy.title', { name: investment.name })}</DialogTitle>
          <DialogDescription>{t('modal.buy.description')}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id="buy-crypto-form"
            onSubmit={form.handleSubmit((values) => buyMutation.mutate(values))}
            className="space-y-4"
          >
            <div className="grid grid-cols-2 gap-3">
              <FormField
                control={form.control}
                name="quantity"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.buy.quantity')}</FormLabel>
                    <InputWrap unit={investment.ticker}>
                      <FormControl>
                        <Input type="number" step="0.00000001" autoFocus {...field} />
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
                    <FormLabel>
                      {t('modal.buy.pricePerUnit', { ticker: investment.ticker })}
                    </FormLabel>
                    <InputWrap unit={currency}>
                      <FormControl>
                        <Input
                          type="number"
                          step="0.01"
                          {...field}
                          onBlur={(e) => {
                            const value = parseFloat(e.target.value);
                            if (!isNaN(value)) field.onChange(value.toFixed(2));
                            field.onBlur();
                          }}
                        />
                      </FormControl>
                    </InputWrap>
                    <FormDescription>{t('modal.buy.rateHint')}</FormDescription>
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
                    <FormLabel>{tc('labels.date')}</FormLabel>
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
              <span>{t('modal.buy.total')}</span>
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
            form="buy-crypto-form"
            disabled={buyMutation.isPending}
            loading={buyMutation.isPending}
          >
            {buyMutation.isPending ? tc('status.adding') : t('modal.buy.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
