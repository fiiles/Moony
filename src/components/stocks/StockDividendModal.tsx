import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useTranslation } from 'react-i18next';
import { requiredNumber } from '@/utils/form-schemas';
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
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Button } from '@/components/ui/button';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { currencyCodeSchema, type CurrencyCode } from '@shared/currencies';
import type { StockDividend } from '@shared/schema';
import { useStockDividendMutations } from '@/hooks/use-stock-dividend-mutations';

// Gross > 0 and 0 ≤ tax ≤ gross are checked again by the backend (spec D8).
const formSchema = z
  .object({
    paidDate: z.string().min(1, 'validation.dateRequired'),
    grossAmount: requiredNumber('validation.amountRequired').positive(
      'validation.dividendGrossPositive'
    ),
    withholdingTax: z.string(),
    currency: currencyCodeSchema,
    note: z.string().max(500, 'validation.dividendNoteTooLong'),
  })
  .superRefine((values, ctx) => {
    if (values.withholdingTax.trim() === '') return;
    const tax = Number(values.withholdingTax);
    if (!Number.isFinite(tax) || tax < 0 || tax > values.grossAmount) {
      ctx.addIssue({
        code: 'custom',
        path: ['withholdingTax'],
        message: 'validation.dividendTaxInvalid',
      });
    }
  });

type FormValues = z.infer<typeof formSchema>;

/** The yyyy-mm-dd value of a date input for a stored UTC-midnight timestamp. */
function toDateInputValue(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toISOString().split('T')[0];
}

function today(): string {
  return new Date().toISOString().split('T')[0];
}

interface StockDividendModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  investmentId: string;
  ticker: string;
  /** Currency the form starts in for a new dividend. */
  defaultCurrency: string;
  /** The dividend to edit; `null` records a new one. */
  dividend: StockDividend | null;
}

/** Record or edit a received dividend: one payout, gross before withholding tax, on its pay date. */
export function StockDividendModal({
  open,
  onOpenChange,
  investmentId,
  ticker,
  defaultCurrency,
  dividend,
}: StockDividendModalProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const { createDividend, updateDividend } = useStockDividendMutations();
  const pending = createDividend.isPending || updateDividend.isPending;

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      paidDate: today(),
      grossAmount: undefined,
      withholdingTax: '',
      currency: defaultCurrency as CurrencyCode,
      note: '',
    },
  });

  useEffect(() => {
    if (!open) return;
    form.reset(
      dividend
        ? {
            paidDate: toDateInputValue(dividend.paidDate),
            grossAmount: Number(dividend.grossAmount),
            withholdingTax: Number(dividend.withholdingTax) > 0 ? dividend.withholdingTax : '',
            currency: dividend.currency as CurrencyCode,
            note: dividend.note ?? '',
          }
        : {
            paidDate: today(),
            grossAmount: undefined,
            withholdingTax: '',
            currency: defaultCurrency as CurrencyCode,
            note: '',
          }
    );
  }, [open, dividend, defaultCurrency, form]);

  function onSubmit(values: FormValues) {
    const data = {
      // A yyyy-mm-dd date input parses as UTC midnight
      paidDate: Math.floor(new Date(values.paidDate).getTime() / 1000),
      grossAmount: values.grossAmount.toString(),
      withholdingTax: values.withholdingTax.trim() === '' ? null : values.withholdingTax.trim(),
      currency: values.currency,
      note: values.note.trim() === '' ? null : values.note.trim(),
    };
    const close = { onSuccess: () => onOpenChange(false) };
    if (dividend) {
      updateDividend.mutate({ id: dividend.id, data }, close);
    } else {
      createDividend.mutate({ investmentId, data }, close);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {dividend
              ? t('detail.dividendRecords.editTitle')
              : t('detail.dividendRecords.addTitle')}
          </DialogTitle>
          <DialogDescription>
            {t('detail.dividendRecords.description', { ticker })}
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="grid gap-4">
            <div className="grid grid-cols-2 gap-4">
              <FormField
                control={form.control}
                name="paidDate"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('detail.dividendRecords.paidDate')} *</FormLabel>
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
            <div className="grid grid-cols-2 gap-4">
              <FormField
                control={form.control}
                name="grossAmount"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('detail.dividendRecords.gross')} *</FormLabel>
                    <FormControl>
                      <Input
                        type="number"
                        step="any"
                        inputMode="decimal"
                        {...field}
                        value={field.value ?? ''}
                      />
                    </FormControl>
                    <FormDescription>{t('detail.dividendRecords.grossHint')}</FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="withholdingTax"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('detail.dividendRecords.tax')}</FormLabel>
                    <FormControl>
                      <Input type="number" step="any" inputMode="decimal" min="0" {...field} />
                    </FormControl>
                    <FormDescription>{t('detail.dividendRecords.taxHint')}</FormDescription>
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
                  <FormLabel>{t('detail.dividendRecords.note')}</FormLabel>
                  <FormControl>
                    <Textarea rows={2} {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <Button type="submit" className="w-full" disabled={pending} loading={pending}>
              {dividend ? t('detail.dividendRecords.save') : t('detail.dividendRecords.add')}
            </Button>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
