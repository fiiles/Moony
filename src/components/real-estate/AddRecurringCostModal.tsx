import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { Plus, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useCurrency } from '@/lib/currency';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { useRealEstateMutations } from '@/hooks/use-real-estate-mutations';
import type { RealEstate } from '@shared/schema';
import { requiredNumber } from '@/utils/form-schemas';

const formSchema = z.object({
  name: z.string().trim().min(1, 'validation.nameRequired'),
  amount: requiredNumber('validation.amountRequired').positive('validation.amountPositive'),
  currency: z.string().min(1, 'validation.currencyRequired'),
  frequency: z.enum(['monthly', 'quarterly', 'yearly']),
});

type FormValues = z.infer<typeof formSchema>;

interface AddRecurringCostModalProps {
  realEstate: RealEstate;
  trigger?: React.ReactNode;
}

/** Small dialog that appends one recurring cost to a property (RED-04). */
export function AddRecurringCostModal({ realEstate, trigger }: AddRecurringCostModalProps) {
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const { currencyCode: userCurrency } = useCurrency();
  const { addRecurringCost } = useRealEstateMutations();
  const [open, setOpen] = useState(false);

  const emptyValues = {
    name: '',
    amount: undefined,
    currency: userCurrency,
    frequency: 'monthly',
  } as unknown as FormValues;

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: emptyValues,
  });

  const handleOpenChange = (next: boolean) => {
    setOpen(next);
    if (next) form.reset(emptyValues);
  };

  const onSubmit = (values: FormValues) => {
    addRecurringCost.mutate(
      {
        realEstate,
        cost: {
          name: values.name.trim(),
          amount: values.amount,
          frequency: values.frequency,
          currency: values.currency,
        },
      },
      { onSuccess: () => setOpen(false) }
    );
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger asChild>
        {trigger ?? (
          <Button size="sm">
            <Plus className="mr-2 h-4 w-4" /> {t('modal.recurringCost.addButton')}
          </Button>
        )}
      </DialogTrigger>
      <DialogContent className="max-w-xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t('modal.recurringCost.title')}</DialogTitle>
          <DialogDescription>{t('modal.recurringCost.description')}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form
            id="add-recurring-cost-form"
            onSubmit={form.handleSubmit(onSubmit)}
            className="space-y-4"
          >
            <FormField
              control={form.control}
              name="name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('modal.add.costName')}</FormLabel>
                  <FormControl>
                    <Input placeholder={t('modal.recurringCost.namePlaceholder')} {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <div className="grid grid-cols-3 gap-4">
              <FormField
                control={form.control}
                name="amount"
                render={({ field }) => (
                  <FormItem className="col-span-1">
                    <FormLabel>{tc('labels.amount')}</FormLabel>
                    <FormControl>
                      <Input
                        type="number"
                        step="0.01"
                        min="0"
                        {...field}
                        value={field.value ?? ''}
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
              <FormField
                control={form.control}
                name="frequency"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('detail.frequency')}</FormLabel>
                    <Select onValueChange={field.onChange} value={field.value}>
                      <FormControl>
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                      </FormControl>
                      <SelectContent>
                        <SelectItem value="monthly">{t('modal.add.monthly')}</SelectItem>
                        <SelectItem value="quarterly">{t('modal.add.quarterly')}</SelectItem>
                        <SelectItem value="yearly">{t('modal.add.yearly')}</SelectItem>
                      </SelectContent>
                    </Select>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>
          </form>
        </Form>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => setOpen(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form="add-recurring-cost-form"
            disabled={addRecurringCost.isPending}
          >
            {addRecurringCost.isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {t('modal.recurringCost.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
