import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
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
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Button } from '@/components/ui/button';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { currencyCodeSchema, type CurrencyCode } from '@shared/currencies';
import type { StockDividend } from '@shared/schema';
import { useStockDividendMutations } from '@/hooks/use-stock-dividend-mutations';

// Gross > 0 is checked again by the backend (spec D8), which also refuses a ticker Yahoo has.
const formSchema = z.object({
  exDate: z.string().min(1, 'validation.dateRequired'),
  grossAmount: requiredNumber('validation.amountRequired').positive(
    'validation.dividendGrossPositive'
  ),
  currency: currencyCodeSchema,
  note: z.string().max(500, 'validation.dividendNoteTooLong'),
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
  /** The manual dividend to edit; `null` enters a new one. */
  dividend: StockDividend | null;
  /** Yahoo Finance has dividends for the ticker: nothing can be entered by hand. */
  fromYahoo: boolean;
}

/**
 * Enter or edit a dividend by hand: one payout, gross before withholding tax, on its ex-date.
 * Only for a ticker Yahoo Finance has no dividends for; otherwise the dialog says so and saves
 * nothing (spec D16).
 */
export function StockDividendModal({
  open,
  onOpenChange,
  investmentId,
  ticker,
  defaultCurrency,
  dividend,
  fromYahoo,
}: StockDividendModalProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const { createDividend, updateDividend } = useStockDividendMutations();
  const pending = createDividend.isPending || updateDividend.isPending;

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      exDate: today(),
      grossAmount: undefined,
      currency: defaultCurrency as CurrencyCode,
      note: '',
    },
  });

  useEffect(() => {
    if (!open) return;
    form.reset(
      dividend
        ? {
            exDate: toDateInputValue(dividend.exDate),
            grossAmount: Number(dividend.grossAmount),
            currency: dividend.currency as CurrencyCode,
            note: dividend.note ?? '',
          }
        : {
            exDate: today(),
            grossAmount: undefined,
            currency: defaultCurrency as CurrencyCode,
            note: '',
          }
    );
  }, [open, dividend, defaultCurrency, form]);

  function onSubmit(values: FormValues) {
    if (fromYahoo) return;
    const data = {
      // A yyyy-mm-dd date input parses as UTC midnight
      exDate: Math.floor(new Date(values.exDate).getTime() / 1000),
      grossAmount: values.grossAmount.toString(),
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
        {fromYahoo && (
          <Alert>
            <Info aria-hidden />
            <AlertTitle>{t('detail.dividendRecords.yahooTitle')}</AlertTitle>
            <AlertDescription>{t('detail.dividendRecords.yahooBody', { ticker })}</AlertDescription>
          </Alert>
        )}
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="grid gap-4">
            <fieldset disabled={fromYahoo} className="grid gap-4 disabled:opacity-60">
              <div className="grid grid-cols-2 gap-4">
                <FormField
                  control={form.control}
                  name="exDate"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('detail.dividendRecords.exDate')} *</FormLabel>
                      <FormControl>
                        <Input type="date" {...field} />
                      </FormControl>
                      <FormDescription>{t('detail.dividendRecords.exDateHint')}</FormDescription>
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
            </fieldset>
            <Button
              type="submit"
              className="w-full"
              disabled={pending || fromYahoo}
              loading={pending}
            >
              {dividend ? t('detail.dividendRecords.save') : t('detail.dividendRecords.add')}
            </Button>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
