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
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input, InputWrap } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { cn } from '@/lib/utils';

const formSchema = z.object({
  quantity: requiredNumber('validation.quantityRequired').positive('validation.quantityPositive'),
  pricePerUnit: requiredNumber('validation.priceRequired').positive('validation.pricePositive'),
  currency: currencyCodeSchema,
  date: z.string().optional(),
});

interface SellCryptoModalProps {
  investment: CryptoHoldingData | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * "Prodat" (prototype crypto-detail.html): quantity with an "all" shortcut,
 * rate in the native currency, and the expected proceeds with the realized
 * result against the average cost.
 */
export function SellCryptoModal({ investment, open, onOpenChange }: SellCryptoModalProps) {
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { convert, currencyCode, formatCurrencyRaw } = useCurrency();
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

  const sellMutation = useMutation({
    mutationFn: async (values: z.infer<typeof formSchema>) => {
      if (!investment) return;
      return cryptoApi.createTransaction(investment.id, {
        type: 'sell',
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
      toast(t('toast.sold'));
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  if (!investment) return null;

  const currency = form.watch('currency') as CurrencyCode;
  const quantity = Number(form.watch('quantity')) || 0;
  const price = Number(form.watch('pricePerUnit')) || 0;
  const priceDisplay = convert(price, currency, currencyCode as CurrencyCode);
  const proceeds = quantity * priceDisplay;
  const realized = quantity * (priceDisplay - investment.avgCost);
  const tooMuch = quantity > investment.quantity;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('modal.sell.title', { name: investment.name })}</DialogTitle>
          <DialogDescription>{t('modal.sell.description')}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id="sell-crypto-form"
            onSubmit={form.handleSubmit((values) => sellMutation.mutate(values))}
            className="space-y-4"
          >
            <div className="grid grid-cols-2 gap-3">
              <FormField
                control={form.control}
                name="quantity"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel className="flex justify-between">
                      <span>{t('modal.sell.quantity')}</span>
                      <button
                        type="button"
                        className="font-500 text-ink-4 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus"
                        onClick={() => field.onChange(investment.quantity.toString())}
                      >
                        {t('modal.sell.all', {
                          quantity: fmt.number(investment.quantity, { maximumFractionDigits: 8 }),
                        })}
                      </button>
                    </FormLabel>
                    <InputWrap unit={investment.ticker}>
                      <FormControl>
                        <Input
                          type="number"
                          step="0.00000001"
                          max={investment.quantity}
                          autoFocus
                          aria-invalid={tooMuch || undefined}
                          {...field}
                        />
                      </FormControl>
                    </InputWrap>
                    {tooMuch && (
                      <p className="text-micro font-600 text-loss">
                        {t('modal.sell.tooMuch', {
                          quantity: fmt.number(investment.quantity, { maximumFractionDigits: 8 }),
                        })}
                      </p>
                    )}
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
                      {t('modal.sell.pricePerUnit', { ticker: investment.ticker })}
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
              <span>
                {proceeds > 0
                  ? t('modal.sell.proceeds', { amount: formatCurrencyRaw(proceeds) })
                  : t('modal.sell.proceedsEmpty')}
              </span>
              <b
                className={cn(
                  'text-[14px] font-650 tracking-[-0.03em] num',
                  proceeds > 0 ? (realized >= 0 ? 'text-gain' : 'text-loss') : 'text-ink'
                )}
              >
                {proceeds > 0
                  ? fmt.money(realized, currencyCode, { signed: true, decimals: 0 })
                  : '—'}
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
            form="sell-crypto-form"
            disabled={sellMutation.isPending || tooMuch}
            loading={sellMutation.isPending}
          >
            {sellMutation.isPending ? tc('status.selling') : t('modal.sell.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
