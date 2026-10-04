import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
import type { CurrencyCode } from '@shared/currencies';
import { requiredNumber } from '@/utils/form-schemas';
import { useFormat } from '@/lib/use-format';
import { useValuationMutations, type ValuationKind } from '@/hooks/use-valuation-mutations';
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
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';

const formSchema = z.object({
  value: requiredNumber('validation.invalidAmount').min(0, 'validation.invalidAmount'),
  valuedAt: z.string().min(1, 'validation.invalidDate'),
  note: z.string().optional(),
});
type FormValues = z.infer<typeof formSchema>;

export interface RevalueAsset {
  id: string;
  name: string;
  /** Today's estimate in the asset's own currency (per unit for other assets). */
  currentValue: number;
  currency: string;
  /** Unit label for per-unit assets ("oz", "ks"); omitted for a whole property. */
  unit?: string;
  /** Held quantity, so the alert can show the resulting total value. */
  quantity?: number;
}

interface RevalueDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  kind: ValuationKind;
  asset: RevalueAsset | null;
}

const todayIso = () => new Date().toISOString().split('T')[0];
const isoToUtcDaySec = (iso: string) => {
  const [y, m, d] = iso.split('-').map(Number);
  return Math.floor(Date.UTC(y, m - 1, d) / 1000);
};

/**
 * "Přecenit" (prototypes real-estate-detail.html, other-assets.html): a new
 * dated estimate; older ones stay in the chart. The alert shows the change
 * against today's estimate and, for per-unit assets, the resulting value.
 */
export function RevalueDialog({ open, onOpenChange, kind, asset }: RevalueDialogProps) {
  const { t } = useTranslation('common');
  const fmt = useFormat();
  const { add } = useValuationMutations(kind, asset?.id);

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: { value: 0, valuedAt: todayIso(), note: '' },
  });

  useEffect(() => {
    if (!open || !asset) return;
    form.reset({ value: asset.currentValue, valuedAt: todayIso(), note: '' });
  }, [open, asset, form]);

  if (!asset) return null;

  const currency = asset.currency as CurrencyCode;
  const value = Number(form.watch('value')) || 0;
  const delta = value - asset.currentValue;
  const deltaPct = asset.currentValue > 0 ? delta / asset.currentValue : 0;
  const total = asset.quantity !== undefined ? value * asset.quantity : value;

  const submit = (values: FormValues) => {
    add.mutate(
      {
        assetId: asset.id,
        value: String(values.value),
        currency: asset.currency,
        valuedAt: isoToUtcDaySec(values.valuedAt),
        note: values.note?.trim() || null,
      },
      { onSuccess: () => onOpenChange(false) }
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{t('revalue.title', { name: asset.name })}</DialogTitle>
          <DialogDescription>{t('revalue.description')}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form id="revalue-form" onSubmit={form.handleSubmit(submit)} className="space-y-4">
            <div className="grid grid-cols-[1fr_150px] gap-3">
              <FormField
                control={form.control}
                name="value"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>
                      {asset.unit
                        ? t('revalue.pricePerUnit', { unit: asset.unit })
                        : t('revalue.marketValue')}
                    </FormLabel>
                    <InputWrap unit={asset.currency}>
                      <FormControl>
                        <Input type="number" step="0.01" min="0" autoFocus {...field} />
                      </FormControl>
                    </InputWrap>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="valuedAt"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('revalue.date')}</FormLabel>
                    <FormControl>
                      <Input type="date" max={todayIso()} {...field} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>
            <FormField
              control={form.control}
              name="note"
              render={({ field }) => (
                <FormItem>
                  <FormLabel className="flex justify-between">
                    <span>{t('revalue.note')}</span>
                    <span className="font-500 text-ink-5">{t('labels.optional')}</span>
                  </FormLabel>
                  <FormControl>
                    <Input placeholder={t('revalue.notePlaceholder')} {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <Alert>
              <Info aria-hidden />
              <AlertTitle>
                {asset.currentValue > 0
                  ? `${fmt.money(delta, currency, { signed: true, decimals: 0 })} · ${fmt.percent(deltaPct, 1, { signed: true })}`
                  : fmt.money(value, currency, { decimals: 0 })}
              </AlertTitle>
              <AlertDescription>
                {asset.quantity !== undefined
                  ? t('revalue.totalNote', {
                      total: fmt.money(total, currency, { decimals: 0 }),
                      previous: fmt.money(asset.currentValue, currency, { decimals: 0 }),
                    })
                  : t('revalue.previousNote', {
                      previous: fmt.money(asset.currentValue, currency, { decimals: 0 }),
                    })}
              </AlertDescription>
            </Alert>
          </form>
        </Form>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {t('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form="revalue-form"
            disabled={add.isPending}
            loading={add.isPending}
          >
            {add.isPending ? t('status.saving') : t('revalue.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
