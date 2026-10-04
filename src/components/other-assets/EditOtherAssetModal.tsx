import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { OtherAsset } from '@shared/schema';
import { currencyCodeSchema, type CurrencyCode } from '@shared/currencies';
import { otherAssetsApi, portfolioApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useCurrency } from '@/lib/currency';
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
  yieldType: z.enum(YIELD_TYPES),
  yieldValue: z.string().optional(),
});
type FormValues = z.infer<typeof formSchema>;

interface EditOtherAssetModalProps {
  asset: OtherAsset | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * Edit an asset's name, currency and yield (the market price has its own
 * "Přecenit" flow, quantity and purchase average come from transactions).
 * The update API existed but had no UI until now.
 */
export function EditOtherAssetModal({ asset, open, onOpenChange }: EditOtherAssetModalProps) {
  const { t } = useTranslation('otherAssets');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { currencyCode } = useCurrency();

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: { name: '', currency: currencyCode, yieldType: 'none', yieldValue: '' },
  });

  useEffect(() => {
    if (!open || !asset) return;
    form.reset({
      name: asset.name,
      currency: (asset.currency || 'CZK') as CurrencyCode,
      yieldType: (YIELD_TYPES as readonly string[]).includes(asset.yieldType)
        ? (asset.yieldType as FormValues['yieldType'])
        : 'none',
      yieldValue: asset.yieldValue ?? '',
    });
  }, [open, asset, form]);

  const mutation = useMutation({
    mutationFn: (values: FormValues) =>
      otherAssetsApi.update(asset!.id, {
        name: values.name.trim(),
        currency: values.currency,
        yieldType: values.yieldType,
        yieldValue: values.yieldType === 'none' ? undefined : values.yieldValue || undefined,
      }),
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
      toast(t('toast.updated'));
      onOpenChange(false);
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  if (!asset) return null;
  const yieldType = form.watch('yieldType');

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('modal.edit.title', { name: asset.name })}</DialogTitle>
          <DialogDescription>{t('modal.edit.description')}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id="edit-other-asset-form"
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
                      <Input autoFocus {...field} />
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
                    <FormDescription>{t('modal.edit.currencyHint')}</FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />
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
                      <InputWrap unit={yieldType === 'fixed' ? form.watch('currency') : '%'}>
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
            <p className="m-0 text-micro font-500 text-ink-4">{t('modal.edit.priceHint')}</p>
          </form>
        </Form>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form="edit-other-asset-form"
            disabled={mutation.isPending}
            loading={mutation.isPending}
          >
            {mutation.isPending ? tc('status.saving') : tc('buttons.saveChanges')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
