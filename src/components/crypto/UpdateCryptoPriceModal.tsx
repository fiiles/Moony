import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { CryptoHoldingData } from '@shared/calculations';
import type { CurrencyCode } from '@shared/currencies';
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

const manualPriceSchema = z.object({
  price: z.string().min(1, 'validation.priceRequired'),
  currency: z.string(),
});

type FormData = z.infer<typeof manualPriceSchema>;

interface UpdateCryptoPriceModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  investment: CryptoHoldingData | null;
}

/**
 * Manual rate (design system §6 Modal): overrides CoinGecko until removed;
 * the footer offers the removal on the left when an override is active.
 */
export function UpdateCryptoPriceModal({
  open,
  onOpenChange,
  investment,
}: UpdateCryptoPriceModalProps) {
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { currencyCode: userCurrency, convert, formatCurrencyRaw } = useCurrency();
  const fmt = useFormat();
  const form = useForm<FormData>({
    resolver: zodResolver(manualPriceSchema),
    defaultValues: { price: '', currency: userCurrency },
  });

  useEffect(() => {
    if (investment && open) {
      const native = Number(investment.originalPrice ?? investment.currentPrice) || 0;
      form.reset({
        price: native ? String(Number(native.toFixed(2))) : '',
        currency: investment.currency || userCurrency,
      });
    }
  }, [investment, open, form, userCurrency]);

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['crypto'] });
    queryClient.invalidateQueries({ queryKey: ['crypto-detail', investment?.id] });
    queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
    queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
  };

  const mutation = useMutation({
    mutationFn: async (data: FormData) => {
      if (!investment) return;
      return cryptoApi.updatePrice(
        investment.ticker,
        data.price,
        data.currency,
        investment.coingeckoId
      );
    },
    onSuccess: () => {
      invalidate();
      toast(t('toast.priceUpdated'));
      onOpenChange(false);
    },
    onError: (error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: async () => {
      if (!investment) return;
      return cryptoApi.deleteManualPrice(investment.ticker);
    },
    onSuccess: () => {
      invalidate();
      toast(t('toast.manualPriceDeleted'));
      onOpenChange(false);
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  if (!investment) return null;

  const price = parseFloat(form.watch('price') || '0') || 0;
  const currency = form.watch('currency') as CurrencyCode;
  const valueDisplay = investment.quantity * convert(price, currency, userCurrency as CurrencyCode);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{t('modal.updatePrice.title', { name: investment.name })}</DialogTitle>
          <DialogDescription>{t('modal.updatePrice.description')}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id="crypto-price-form"
            onSubmit={form.handleSubmit((data) => mutation.mutate(data))}
            className="space-y-4"
          >
            <div className="grid grid-cols-[1fr_138px] gap-3">
              <FormField
                control={form.control}
                name="price"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>
                      {t('modal.updatePrice.price', { ticker: investment.ticker })}
                    </FormLabel>
                    <InputWrap unit={currency}>
                      <FormControl>
                        <Input type="number" step="0.01" placeholder="0" autoFocus {...field} />
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
            <div className="flex items-center justify-between border-t border-line-soft pt-3 text-caption text-ink-3">
              <span>
                {t('modal.updatePrice.total', {
                  quantity: fmt.number(investment.quantity, { maximumFractionDigits: 8 }),
                  ticker: investment.ticker,
                })}
              </span>
              <b className="text-[14px] font-650 tracking-[-0.03em] text-ink num">
                {price > 0 ? formatCurrencyRaw(valueDisplay) : '—'}
              </b>
            </div>
          </form>
        </Form>
        <DialogFooter className={investment.isManualPrice ? 'sm:justify-between' : undefined}>
          {investment.isManualPrice && (
            <Button
              type="button"
              variant="danger"
              onClick={() => deleteMutation.mutate()}
              disabled={deleteMutation.isPending}
              loading={deleteMutation.isPending}
            >
              {t('modal.updatePrice.remove')}
            </Button>
          )}
          <div className="flex gap-2">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {tc('buttons.cancel')}
            </Button>
            <Button
              type="submit"
              form="crypto-price-form"
              disabled={mutation.isPending}
              loading={mutation.isPending}
            >
              {mutation.isPending ? tc('status.saving') : t('modal.updatePrice.submit')}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
