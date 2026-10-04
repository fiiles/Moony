import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
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
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { currencyCodeSchema } from '@shared/currencies';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import type { InvestmentTransaction } from '@shared/schema';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useInvestmentTransactionMutations } from '@/hooks/use-investment-mutations';
import { FormSection } from '@/components/ui/form-section';

const formSchema = z.object({
  quantity: requiredNumber('validation.quantityRequired').positive('validation.quantityPositive'),
  pricePerUnit: requiredNumber('validation.priceRequired').positive('validation.pricePositive'),
  currency: currencyCodeSchema,
  date: z.string().min(1, 'validation.dateRequired'),
});

/** The yyyy-mm-dd value shown in the date input for a stored unix timestamp. */
function toDateInputValue(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toISOString().split('T')[0];
}

interface EditTransactionModalProps {
  transaction: InvestmentTransaction | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function EditTransactionModal({
  transaction,
  open,
  onOpenChange,
}: EditTransactionModalProps) {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const { updateTransaction } = useInvestmentTransactionMutations();

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      quantity: 0,
      pricePerUnit: 0,
      currency: 'USD',
      date: new Date().toISOString().split('T')[0],
    },
  });

  // Reset form when transaction changes
  useEffect(() => {
    if (transaction) {
      form.reset({
        quantity: parseFloat(transaction.quantity),
        pricePerUnit: parseFloat(transaction.pricePerUnit),
        currency: transaction.currency as z.infer<typeof formSchema>['currency'],
        date: toDateInputValue(transaction.transactionDate),
      });
    }
  }, [transaction, form]);

  function onSubmit(values: z.infer<typeof formSchema>) {
    if (!transaction) return;

    // An untouched date keeps its exact stored timestamp: the date input only has
    // day resolution, so re-deriving it could shift a transaction by a day.
    const transactionDate =
      values.date === toDateInputValue(transaction.transactionDate)
        ? transaction.transactionDate
        : Math.floor(new Date(values.date).getTime() / 1000);

    updateTransaction.mutate(
      {
        txId: transaction.id,
        data: {
          type: transaction.type, // Type is read-only
          ticker: transaction.ticker,
          companyName: transaction.companyName,
          quantity: values.quantity.toString(),
          pricePerUnit: values.pricePerUnit.toString(),
          currency: values.currency,
          transactionDate,
        },
      },
      { onSuccess: () => onOpenChange(false) }
    );
  }

  if (!transaction) return null;

  const typeLabel = transaction.type === 'buy' ? tc('buttons.buy') : tc('buttons.sell');

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t('modal.edit.title', { type: typeLabel })}</DialogTitle>
          <DialogDescription>{t('modal.edit.description')}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-6">
            <FormSection title={t('modal.edit.transactionDetails')} first>
              <div className="grid gap-4">
                <div className="grid grid-cols-2 gap-4">
                  <FormField
                    control={form.control}
                    name="pricePerUnit"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('modal.buy.pricePerShare')} *</FormLabel>
                        <FormControl>
                          <Input type="number" step="0.01" {...field} />
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
                    name="quantity"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('modal.buy.quantity')} *</FormLabel>
                        <FormControl>
                          <Input type="number" step="0.0001" {...field} />
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
                      </FormItem>
                    )}
                  />
                </div>
              </div>
            </FormSection>

            <Button type="submit" className="w-full" disabled={updateTransaction.isPending}>
              {updateTransaction.isPending ? tc('status.updating') : t('modal.edit.submit')}
            </Button>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
