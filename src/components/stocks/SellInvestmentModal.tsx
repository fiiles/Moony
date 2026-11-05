import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { requiredNumber, optionalFee, feeForCreate } from '@/utils/form-schemas';
import { useMutation } from '@tanstack/react-query';
import { useRefreshAfterStockTransaction } from '@/hooks/use-investment-mutations';
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
import { Input, InputWrap } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { currencyCodeSchema, type CurrencyCode } from '@shared/currencies';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import type { HoldingData } from '@/utils/stocks';
import { tradeCurrency } from '@/utils/positions';
import { investmentsApi } from '@/lib/tauri-api';
import { useTranslation } from 'react-i18next';
import { PreSplitHint } from '@/components/stocks/PreSplitHint';
import { FormSection } from '@/components/ui/form-section';

const formSchema = z.object({
  quantity: requiredNumber('validation.quantityRequired').positive('validation.quantityPositive'),
  pricePerUnit: requiredNumber('validation.priceRequired').positive('validation.pricePositive'),
  currency: currencyCodeSchema,
  fee: optionalFee(),
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
  const refreshAfterChange = useRefreshAfterStockTransaction();

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      quantity: 0,
      pricePerUnit: 0,
      currency: 'USD',
      fee: '',
      date: new Date().toISOString().split('T')[0],
    },
  });

  // Lock currency to the investment's base currency (set by first transaction)
  useEffect(() => {
    if (investment) {
      form.setValue('currency', tradeCurrency(investment) as CurrencyCode);
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
        fee: feeForCreate(values.fee),
        transactionDate: values.date
          ? Math.floor(new Date(values.date).getTime() / 1000)
          : Math.floor(Date.now() / 1000),
      };

      return investmentsApi.createTransaction(investment.id, txData);
    },
    onSuccess: async () => {
      onOpenChange(false);
      form.reset();
      await refreshAfterChange();
    },
  });

  const currency = form.watch('currency');

  function onSubmit(values: z.infer<typeof formSchema>) {
    sellInvestment.mutate(values);
  }

  if (!investment) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t('modal.sell.titleNamed', { name: investment.ticker })}</DialogTitle>
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
                          <Input type="number" step="any" {...field} />
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
                  name="fee"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.fee')}</FormLabel>
                      <InputWrap unit={currency}>
                        <FormControl>
                          <Input type="number" step="any" min="0" placeholder="0.00" {...field} />
                        </FormControl>
                      </InputWrap>
                      <FormMessage />
                    </FormItem>
                  )}
                />
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
                        <PreSplitHint ticker={investment.ticker} date={field.value} />
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
