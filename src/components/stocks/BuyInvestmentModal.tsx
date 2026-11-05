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
import { currencyCodeSchema, type CurrencyCode } from '@shared/currencies';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { investmentsApi } from '@/lib/tauri-api';
import { useEffect } from 'react';
import type { HoldingData } from '@/utils/stocks';
import { tradeCurrency, tradePricePrefill } from '@/utils/positions';
import { useTranslation } from 'react-i18next';
import { PreSplitHint } from '@/components/stocks/PreSplitHint';

const formSchema = z.object({
  quantity: requiredNumber('validation.quantityRequired').positive('validation.quantityPositive'),
  pricePerUnit: requiredNumber('validation.priceRequired').positive('validation.pricePositive'),
  currency: currencyCodeSchema,
  fee: optionalFee(),
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

  // Update default currency and price when investment changes
  // Currency is locked to the investment's base currency (set by first transaction)
  useEffect(() => {
    if (investment) {
      form.setValue('currency', tradeCurrency(investment) as CurrencyCode);
      // The price is prefilled only when it is quoted in the locked currency
      form.setValue('pricePerUnit', tradePricePrefill(investment));
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
    buyMutation.mutate(values);
  }

  if (!investment) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('modal.buy.titleNamed', { name: investment.companyName })}</DialogTitle>
          <DialogDescription>
            {t('modal.buy.descriptionFor', { ticker: investment.ticker })}
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
