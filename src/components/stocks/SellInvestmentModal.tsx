import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { requiredNumber } from '@/utils/form-schemas';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import {
  Dialog,
  DialogContent,
  DialogDescription,
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
import type { HoldingData } from '@/utils/stocks';
import { investmentsApi } from '@/lib/tauri-api';
import { useTranslation } from 'react-i18next';
import { FormSection } from '@/components/ui/form-section';

const formSchema = z.object({
  quantity: requiredNumber('validation.quantityRequired').positive('validation.quantityPositive'),
  pricePerUnit: requiredNumber('validation.priceRequired').positive('validation.pricePositive'),
  currency: currencyCodeSchema,
  date: z.string().optional(),
});

interface SellInvestmentModalProps {
  investment: HoldingData | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SellInvestmentModal({ investment, open, onOpenChange }: SellInvestmentModalProps) {
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

  // Lock currency to the investment's base currency (set by first transaction)
  useEffect(() => {
    if (investment) {
      const investmentCurrency = (investment.avgCostCurrency || investment.currency || 'USD') as
        'USD' | 'EUR' | 'CZK';
      form.setValue('currency', investmentCurrency);
    }
  }, [investment, form]);

  const sellInvestment = useMutation({
    mutationFn: async (values: z.infer<typeof formSchema>) => {
      if (!investment) return;

      const txData = {
        type: 'sell',
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
    sellInvestment.mutate(values);
  }

  if (!investment) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {t('modal.sell.title')} {investment.ticker}
          </DialogTitle>
          <DialogDescription>{t('modal.sell.description')}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-6">
            <FormSection title={t('modal.sell.transactionDetails')} first>
              <div className="grid gap-4">
                <div className="grid grid-cols-2 gap-4">
                  <FormField
                    control={form.control}
                    name="pricePerUnit"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('modal.sell.pricePerShare')} *</FormLabel>
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
                <div className="grid grid-cols-2 gap-4">
                  <FormField
                    control={form.control}
                    name="quantity"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>
                          {t('modal.sell.quantity')} * ({tc('labels.max')}: {investment.quantity})
                        </FormLabel>
                        <FormControl>
                          <Input type="number" step="0.0001" max={investment.quantity} {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
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
                </div>
              </div>
            </FormSection>

            <Button type="submit" className="w-full" disabled={sellInvestment.isPending}>
              {sellInvestment.isPending ? tc('status.selling') : t('modal.sell.sellInvestment')}
            </Button>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
