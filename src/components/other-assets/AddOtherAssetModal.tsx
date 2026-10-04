import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { currencyCodeSchema, type CurrencyCode } from '@shared/currencies';
import { otherAssetsApi, portfolioApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';

const YIELD_TYPES = ['none', 'fixed', 'percent_purchase', 'percent_market'] as const;

const formSchema = z.object({
  name: z.string().min(1, 'validation.nameRequired'),
  currency: currencyCodeSchema,
  marketPrice: z.string().min(1, 'validation.invalidAmount'),
  initialQuantity: z.string().optional(),
  initialPrice: z.string().optional(),
  initialDate: z.string().optional(),
  yieldType: z.enum(YIELD_TYPES),
  yieldValue: z.string().optional(),
});
type FormValues = z.infer<typeof formSchema>;

interface AddOtherAssetModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * "Přidat aktivum" (design system §6 Modal): name and currency, today's price
 * per unit, an optional first purchase (quantity, price, date) that becomes
 * the first transaction, and the yield type. The summary line shows the
 * resulting value.
 */
export function AddOtherAssetModal({ open, onOpenChange }: AddOtherAssetModalProps) {
  const { t } = useTranslation('otherAssets');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const fmt = useFormat();

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      name: '',
      currency: 'CZK',
      marketPrice: '',
      initialQuantity: '',
      initialPrice: '',
      initialDate: new Date().toISOString().split('T')[0],
      yieldType: 'none',
      yieldValue: '',
    },
  });

  const mutation = useMutation({
    mutationFn: async (data: FormValues) => {
      const hasPurchase = !!(data.initialQuantity && data.initialPrice && data.initialDate);
      const initialTransaction = hasPurchase
        ? {
            type: 'buy',
            quantity: data.initialQuantity!,
            pricePerUnit: data.initialPrice!,
            currency: data.currency,
            transactionDate: Math.floor(new Date(data.initialDate!).getTime() / 1000),
          }
        : undefined;
      return otherAssetsApi.create(
        {
          name: data.name.trim(),
          marketPrice: data.marketPrice,
          currency: data.currency,
          yieldType: data.yieldType,
          yieldValue: data.yieldType === 'none' || !data.yieldValue ? undefined : data.yieldValue,
          quantity: hasPurchase ? data.initialQuantity : '0',
          averagePurchasePrice: hasPurchase ? data.initialPrice : '0',
        },
        initialTransaction
      );
    },
    onSuccess: async () => {
      queryClient.invalidateQueries({ queryKey: ['other-assets'] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      try {
        await portfolioApi.recordSnapshot();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (error) {
        console.error('Failed to record portfolio snapshot:', error);
      }
      toast(t('toast.added'));
      onOpenChange(false);
      form.reset();
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  const currency = form.watch('currency') as CurrencyCode;
  const yieldType = form.watch('yieldType');
  const quantity = parseFloat(form.watch('initialQuantity') || '') || 0;
  const price = parseFloat(form.watch('marketPrice') || '') || 0;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t('modal.add.title')}</DialogTitle>
          <DialogDescription>{t('modal.add.description')}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id="add-other-asset-form"
            onSubmit={form.handleSubmit((values) => mutation.mutate(values))}
            className="space-y-4"
          >
            <div className="grid grid-cols-[1fr_138px] gap-3">
              <FormField
                control={form.control}
                name="name"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.assetName')}</FormLabel>
                    <FormControl>
                      <Input
                        placeholder={t('modal.add.assetNamePlaceholder')}
                        autoFocus
                        {...field}
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
            <FormField
              control={form.control}
              name="marketPrice"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('modal.add.marketPrice')}</FormLabel>
                  <InputWrap unit={currency}>
                    <FormControl>
                      <Input type="number" step="0.01" min="0" placeholder="0" {...field} />
                    </FormControl>
                  </InputWrap>
                  <FormDescription>{t('modal.add.marketPriceHint')}</FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <div>
              <p className="mb-2 text-caption font-700 text-ink-2">
                {t('modal.add.initialPurchase')}
              </p>
              <div className="grid grid-cols-3 gap-3">
                <FormField
                  control={form.control}
                  name="initialQuantity"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{tc('labels.quantity')}</FormLabel>
                      <InputWrap unit={t('table.unit')}>
                        <FormControl>
                          <Input type="number" step="0.0001" min="0" {...field} />
                        </FormControl>
                      </InputWrap>
                      <FormMessage />
                    </FormItem>
                  )}
                />
                <FormField
                  control={form.control}
                  name="initialPrice"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.add.pricePerUnit')}</FormLabel>
                      <InputWrap unit={currency}>
                        <FormControl>
                          <Input type="number" step="0.01" min="0" {...field} />
                        </FormControl>
                      </InputWrap>
                      <FormMessage />
                    </FormItem>
                  )}
                />
                <FormField
                  control={form.control}
                  name="initialDate"
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
              <p className="mt-1.5 text-micro font-500 text-ink-4">
                {t('modal.add.initialPurchaseHint')}
              </p>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <FormField
                control={form.control}
                name="yieldType"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('modal.add.yieldType')}</FormLabel>
                    <Select value={field.value} onValueChange={field.onChange}>
                      <FormControl>
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                      </FormControl>
                      <SelectContent>
                        <SelectItem value="none">{t('modal.add.yieldTypes.none')}</SelectItem>
                        <SelectItem value="fixed">{t('modal.add.yieldTypes.fixed')}</SelectItem>
                        <SelectItem value="percent_purchase">
                          {t('modal.add.yieldTypes.percentPurchase')}
                        </SelectItem>
                        <SelectItem value="percent_market">
                          {t('modal.add.yieldTypes.percentMarket')}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                    <FormMessage />
                  </FormItem>
                )}
              />
              {yieldType !== 'none' && (
                <FormField
                  control={form.control}
                  name="yieldValue"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('modal.add.yieldValue')}</FormLabel>
                      <InputWrap unit={yieldType === 'fixed' ? currency : '%'}>
                        <FormControl>
                          <Input type="number" step="any" min="0" {...field} />
                        </FormControl>
                      </InputWrap>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              )}
            </div>
            <div className="flex items-center justify-between border-t border-line-soft pt-3 text-caption text-ink-3">
              <span>{t('modal.add.total')}</span>
              <b className="text-[14px] font-650 tracking-[-0.03em] text-ink num">
                {quantity > 0 && price > 0
                  ? fmt.money(quantity * price, currency, { decimals: 0 })
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
            form="add-other-asset-form"
            disabled={mutation.isPending}
            loading={mutation.isPending}
          >
            {mutation.isPending ? tc('status.adding') : t('modal.add.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
