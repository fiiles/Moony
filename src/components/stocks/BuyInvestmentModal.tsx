import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { requiredNumber } from '@/utils/form-schemas';
import { useMutation, useQueryClient } from '@tanstack/react-query';
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
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { currencyCodeSchema } from '@shared/currencies';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { investmentsApi } from '@/lib/tauri-api';
import { useEffect } from 'react';
import type { HoldingData } from '@/utils/stocks';
import { useTranslation } from 'react-i18next';

const formSchema = z.object({
  quantity: requiredNumber('validation.quantityRequired').positive('validation.quantityPositive'),
  pricePerUnit: requiredNumber('validation.priceRequired').positive('validation.pricePositive'),
  currency: currencyCodeSchema,
  date: z.string().optional(),
});

interface BuyInvestmentModalProps {
  investment: HoldingData | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function BuyInvestmentModal({ investment, open, onOpenChange }: BuyInvestmentModalProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      quantity: 0,
      pricePerUnit: 0,
      currency: 'USD',
      date: new Date().toISOString().split('T')[0],
    },
  });

  // Update default currency and price when investment changes
  // Currency is locked to the investment's base currency (set by first transaction)
  useEffect(() => {
    if (investment) {
      // Use avgCostCurrency (investment's locked currency) for transactions
      const investmentCurrency = (investment.avgCostCurrency || investment.currency || 'USD') as
        'USD' | 'EUR' | 'CZK';
      form.setValue('currency', investmentCurrency);
      // Use originalPrice (in stock's currency) for prefill, not currentPrice (which is converted to CZK)
      const priceToShow = Number(investment.originalPrice ?? investment.currentPrice);
      form.setValue('pricePerUnit', priceToShow ? Number(priceToShow.toFixed(2)) : 0);
    }
  }, [investment, form]);

  const buyMutation = useMutation({
    mutationFn: async (values: z.infer<typeof formSchema>) => {
      if (!investment) return;

      const txData = {
        type: 'buy',
        ticker: investment.ticker,
        companyName: investment.companyName,
        quantity: values.quantity.toString(),
        pricePerUnit: values.pricePerUnit.toString(),
        currency: values.currency,
        transactionDate: values.date
          ? Math.floor(new Date(values.date).getTime() / 1000)
          : Math.floor(Date.now() / 1000),
      };

      return investmentsApi.createTransaction(investment.id, txData);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['investment', investment?.id] });
      queryClient.invalidateQueries({ queryKey: ['investment-transactions', investment?.id] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['all-stock-transactions'] });
      onOpenChange(false);
      form.reset();
    },
  });

  function onSubmit(values: z.infer<typeof formSchema>) {
    buyMutation.mutate(values);
  }

  if (!investment) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>
            {t('modal.buy.title')} {investment.companyName}
          </DialogTitle>
          <DialogDescription>
            {t('modal.buy.description')} {investment.ticker}.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id="buy-investment-form"
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-4"
          >
            <FormField
              control={form.control}
              name="quantity"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('modal.buy.quantity')} *</FormLabel>
                  <FormControl>
                    <Input type="number" step="any" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <div className="grid grid-cols-2 gap-4">
              <FormField
                control={form.control}
                name="pricePerUnit"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.buy.pricePerShare')} *</FormLabel>
                    <FormControl>
                      <Input
                        type="number"
                        step="0.01"
                        {...field}
                        onBlur={(e) => {
                          const value = parseFloat(e.target.value);
                          if (!isNaN(value)) {
                            field.onChange(value.toFixed(2));
                          }
                          field.onBlur();
                        }}
                      />
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
                      <CurrencyCombobox
                        value={field.value}
                        onChange={field.onChange}
                        disabled
                        className="bg-well"
                      />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>
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
          </form>
        </Form>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button type="submit" form="buy-investment-form" disabled={buyMutation.isPending}>
            {buyMutation.isPending ? tc('status.adding') : t('modal.buy.addPurchase')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
