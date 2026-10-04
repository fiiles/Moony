import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
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
import { toast } from 'sonner';
import { HoldingData } from '@/utils/stocks';
import { useCurrency } from '@/lib/currency';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { useFormat } from '@/lib/use-format';
import type { CurrencyCode } from '@shared/currencies';
import { investmentsApi } from '@/lib/tauri-api';
import { useTranslation } from 'react-i18next';

const manualPriceSchema = z.object({
  price: z.string().min(1, 'validation.priceRequired'),
  currency: z.string(),
});

type FormData = z.infer<typeof manualPriceSchema>;

interface ManualPriceModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  investment: HoldingData | null;
}

export function ManualPriceModal({ open, onOpenChange, investment }: ManualPriceModalProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { currencyCode: userCurrency, convert } = useCurrency();
  const fmt = useFormat();
  const form = useForm<FormData>({
    resolver: zodResolver(manualPriceSchema),
    defaultValues: {
      price: '',
      currency: userCurrency,
    },
  });

  // Reset form when investment changes
  useEffect(() => {
    if (investment && open) {
      // Use originalPrice (in stock's currency) for prefill, not currentPrice (which is converted to CZK)
      const priceToShow = Number(investment.originalPrice ?? investment.currentPrice);
      form.reset({
        price: priceToShow ? Math.round(priceToShow).toString() : '',
        currency: investment.currency || userCurrency,
      });
    }
  }, [investment, open, form, userCurrency]);

  const mutation = useMutation({
    mutationFn: async (data: FormData) => {
      if (!investment) return;
      return investmentsApi.setManualPrice(investment.ticker, data.price, data.currency);
    },

    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['investment', investment?.id] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      toast(tc('status.success'), { description: t('toast.updated') });
      onOpenChange(false);
    },
    onError: (error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: async () => {
      if (!investment) return;
      return investmentsApi.deleteManualPrice(investment.ticker);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['investment', investment?.id] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      toast(tc('status.success'), { description: t('toast.manualPriceDeleted') });
      onOpenChange(false);
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  const onSubmit = (data: FormData) => {
    mutation.mutate(data);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('modals.updatePrice.title')}</DialogTitle>
          <DialogDescription>{t('modals.updatePrice.description')}</DialogDescription>
        </DialogHeader>

        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            <div className="grid grid-cols-2 gap-4">
              <FormField
                control={form.control}
                name="price"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{tc('labels.price')}</FormLabel>
                    <FormControl>
                      <Input
                        type="number"
                        step="1"
                        placeholder="0"
                        {...field}
                        onBlur={(e) => {
                          const value = parseFloat(e.target.value);
                          if (!isNaN(value)) {
                            field.onChange(Math.round(value).toString());
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
                      <CurrencyCombobox value={field.value} onChange={field.onChange} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>

            <div className="rounded-r2 bg-well p-3 text-sm">
              <div className="flex justify-between items-center">
                <span className="text-ink-3">{tc('labels.estimatedTotal')}</span>
                <span className="font-medium">
                  {(() => {
                    const price = parseFloat(form.watch('price') || '0');
                    const quantity = investment?.quantity || 0;
                    const total = price * quantity;
                    const formCurrency = form.watch('currency') as CurrencyCode;

                    if (formCurrency !== userCurrency) {
                      const convertedTotal = convert(total, formCurrency, userCurrency);
                      return `${fmt.money(total, formCurrency)} / ${fmt.money(convertedTotal, userCurrency)}`;
                    }

                    return fmt.money(total, formCurrency);
                  })()}
                </span>
              </div>
              <div className="text-xs text-ink-3 mt-1 text-right">
                {t('modals.manualFormula', {
                  value: form.watch('price') || '0',
                  quantity: investment?.quantity?.toFixed(4) || '0',
                })}
              </div>
            </div>

            <div className="flex gap-2">
              <Button type="submit" className="flex-1" disabled={mutation.isPending}>
                {mutation.isPending ? tc('status.updating') : t('actions.updatePrice')}
              </Button>
              {investment?.isManualPrice && (
                <Button
                  type="button"
                  variant="destructive"
                  onClick={() => deleteMutation.mutate()}
                  disabled={deleteMutation.isPending}
                >
                  {deleteMutation.isPending
                    ? tc('status.deleting', { defaultValue: 'Deleting...' })
                    : tc('actions.delete', { defaultValue: 'Delete' })}
                </Button>
              )}
            </div>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
