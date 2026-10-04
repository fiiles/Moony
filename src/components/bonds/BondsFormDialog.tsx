import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
import type { Bond, InsertBond } from '@shared/schema';
import { currencyCodeSchema, type CurrencyCode } from '@shared/currencies';
import { requiredNumber } from '@/utils/form-schemas';
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
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input, InputWrap } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';

const formSchema = z.object({
  name: z.string().min(1, 'validation.nameRequired'),
  isin: z.string().optional(),
  couponValue: requiredNumber('validation.invalidAmount').positive('validation.invalidAmount'),
  quantity: requiredNumber('validation.quantityRequired')
    .int('validation.quantityPositive')
    .min(1, 'validation.quantityPositive'),
  currency: currencyCodeSchema,
  interestRate: requiredNumber('validation.interestRatePositive')
    .min(0, 'validation.interestRatePositive')
    .max(100, 'validation.interestRatePositive'),
  maturityDate: z.string().optional(),
});
type FormValues = z.infer<typeof formSchema>;

interface BondsFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: InsertBond) => void;
  bond?: Bond | null;
  isLoading?: boolean;
}

const isoDay = (sec: number) => new Date(sec * 1000).toISOString().split('T')[0];
/** 'YYYY-MM-DD' from a date input → UTC-midnight unix seconds (ADR 0008). */
const isoToUtcDaySec = (iso: string) => {
  const [y, m, d] = iso.split('-').map(Number);
  return Math.floor(Date.UTC(y, m - 1, d) / 1000);
};

/**
 * Add / edit a bond (design system §6 Modal, prototype bonds.html): name and
 * ISIN, face value with currency, count and yearly coupon, maturity; a
 * summary alert shows the value, the yearly coupon and the next anniversary.
 * Every field is editable in both modes (the old edit form could not change
 * quantity, ISIN or currency).
 */
export function BondsFormDialog({
  open,
  onOpenChange,
  onSubmit,
  bond,
  isLoading = false,
}: BondsFormDialogProps) {
  const { t } = useTranslation('bonds');
  const { t: tc } = useTranslation('common');
  const { currencyCode } = useCurrency();
  const fmt = useFormat();
  const isEdit = !!bond;

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      name: '',
      isin: '',
      couponValue: 0,
      quantity: 1,
      currency: currencyCode as CurrencyCode,
      interestRate: 0,
      maturityDate: '',
    },
  });

  useEffect(() => {
    if (!open) return;
    form.reset(
      bond
        ? {
            name: bond.name,
            isin: bond.isin ?? '',
            couponValue: parseFloat(bond.couponValue) || 0,
            quantity: parseFloat(bond.quantity) || 1,
            currency: (bond.currency || 'CZK') as CurrencyCode,
            interestRate: parseFloat(bond.interestRate) || 0,
            maturityDate: bond.maturityDate ? isoDay(bond.maturityDate) : '',
          }
        : {
            name: '',
            isin: '',
            couponValue: 0,
            quantity: 1,
            currency: currencyCode as CurrencyCode,
            interestRate: 0,
            maturityDate: '',
          }
    );
  }, [open, bond, currencyCode, form]);

  const currency = form.watch('currency') as CurrencyCode;
  const couponValue = Number(form.watch('couponValue')) || 0;
  const quantity = Number(form.watch('quantity')) || 0;
  const interestRate = Number(form.watch('interestRate')) || 0;
  const maturity = form.watch('maturityDate');
  const value = couponValue * quantity;
  const yearlyCoupon = (value * interestRate) / 100;
  const nextAnniversary = (() => {
    if (!maturity) return null;
    const m = new Date(maturity);
    if (Number.isNaN(m.getTime())) return null;
    const now = new Date();
    let candidate = Date.UTC(now.getUTCFullYear(), m.getUTCMonth(), m.getUTCDate());
    if (candidate <= now.getTime()) {
      candidate = Date.UTC(now.getUTCFullYear() + 1, m.getUTCMonth(), m.getUTCDate());
    }
    return candidate > m.getTime() ? null : Math.floor(candidate / 1000);
  })();

  const submit = (values: FormValues) => {
    onSubmit({
      name: values.name.trim(),
      isin: values.isin?.trim() ? values.isin.trim().toUpperCase() : null,
      couponValue: String(values.couponValue),
      quantity: String(values.quantity),
      currency: values.currency,
      interestRate: String(values.interestRate),
      maturityDate: values.maturityDate ? isoToUtcDaySec(values.maturityDate) : undefined,
    } as InsertBond);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{isEdit ? t('form.editTitle') : t('form.addTitle')}</DialogTitle>
          <DialogDescription>
            {isEdit ? t('form.editDescription') : t('form.addDescription')}
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form id="bond-form" onSubmit={form.handleSubmit(submit)} className="space-y-4">
            <FormField
              control={form.control}
              name="name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('form.name')}</FormLabel>
                  <FormControl>
                    <Input placeholder={t('form.namePlaceholder')} autoFocus {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="isin"
              render={({ field }) => (
                <FormItem>
                  <FormLabel className="flex justify-between">
                    <span>{t('form.isin')}</span>
                    <span className="font-500 text-ink-5">{tc('labels.optional')}</span>
                  </FormLabel>
                  <FormControl>
                    <Input placeholder="CZ0001…" className="num uppercase" {...field} />
                  </FormControl>
                  <FormDescription>{t('form.isinHint')}</FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <div className="grid grid-cols-[1fr_138px] gap-3">
              <FormField
                control={form.control}
                name="couponValue"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('form.nominal')}</FormLabel>
                    <FormControl>
                      <Input type="number" step="0.01" min="0" {...field} />
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
            <div className="grid grid-cols-2 gap-3">
              <FormField
                control={form.control}
                name="quantity"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('form.quantity')}</FormLabel>
                    <InputWrap unit={t('table.units')}>
                      <FormControl>
                        <Input type="number" step="1" min="1" {...field} />
                      </FormControl>
                    </InputWrap>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="interestRate"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('form.rate')}</FormLabel>
                    <InputWrap unit="%">
                      <FormControl>
                        <Input type="number" step="0.01" min="0" max="100" {...field} />
                      </FormControl>
                    </InputWrap>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>
            <FormField
              control={form.control}
              name="maturityDate"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('form.maturity')}</FormLabel>
                  <FormControl>
                    <Input type="date" {...field} />
                  </FormControl>
                  <FormDescription>{t('form.maturityHint')}</FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            {value > 0 && (
              <Alert>
                <Info aria-hidden />
                <AlertTitle>
                  {t('form.summaryValue', {
                    value: fmt.money(value, currency),
                    coupon: fmt.money(yearlyCoupon, currency),
                  })}
                </AlertTitle>
                <AlertDescription>
                  {nextAnniversary
                    ? t('form.summaryNext', { date: fmt.day(nextAnniversary) })
                    : maturity
                      ? t('form.summaryMatures')
                      : t('form.summaryNoMaturity')}
                </AlertDescription>
              </Alert>
            )}
          </form>
        </Form>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button type="submit" form="bond-form" disabled={isLoading} loading={isLoading}>
            {isLoading ? tc('status.saving') : isEdit ? t('form.submitEdit') : t('form.submitAdd')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
