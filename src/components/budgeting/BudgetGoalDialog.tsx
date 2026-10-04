import { useEffect, useMemo } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { TransactionCategory } from '@shared/schema';
import type { CurrencyCode } from '@shared/currencies';
import { budgetingApi, type BudgetGoal, type InsertBudgetGoal } from '@/lib/tauri-api';
import { requiredNumber } from '@/utils/form-schemas';
import { getUtcPeriodRange } from '@/utils/period';
import { useCurrency } from '@/lib/currency';
import { useCategoryName } from '@/hooks/use-categories';
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

const AVERAGE_MONTHS = 6;

const formSchema = z.object({
  categoryId: z.string().min(1, 'validation.categoryIdRequired'),
  amount: requiredNumber('validation.amountRequired').positive('validation.amountPositive'),
});

type FormValues = z.infer<typeof formSchema>;

export interface BudgetGoalDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Editing: the category is fixed and the current limit is prefilled. */
  goal?: BudgetGoal | null;
  /** Adding from an unbudgeted row: that category is preselected. */
  initialCategoryId?: string | null;
  /** Expense categories that have no limit yet (the add case). */
  availableCategories: TransactionCategory[];
  /** All categories, to name the one being edited. */
  categories: TransactionCategory[];
}

/**
 * "Přidat rozpočet" / "Upravit limit" (design system §6 Modal, prototype
 * `budgets.html`): category select, monthly limit with the display currency
 * as unit and the category's average spending over the last six full months
 * as the hint. Limits are stored monthly and recur on their own.
 */
export function BudgetGoalDialog({
  open,
  onOpenChange,
  goal,
  initialCategoryId,
  availableCategories,
  categories,
}: BudgetGoalDialogProps) {
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  const { currencyCode, convert, formatCurrency } = useCurrency();
  const categoryName = useCategoryName();
  const queryClient = useQueryClient();
  const editing = !!goal;

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: { categoryId: '', amount: 0 },
  });

  // Reset for each opening: edited limit in the display currency, or the
  // preselected category with an empty amount.
  useEffect(() => {
    if (!open) return;
    if (goal) {
      const inDisplay = convert(
        parseFloat(goal.amount) || 0,
        goal.currency as CurrencyCode,
        currencyCode
      );
      form.reset({ categoryId: goal.categoryId, amount: Math.round(inDisplay * 100) / 100 });
    } else {
      form.reset({ categoryId: initialCategoryId ?? '', amount: undefined });
    }
  }, [open, goal, initialCategoryId, convert, currencyCode, form]);

  // Average monthly spending per category over the last six full months (CZK).
  const averageRange = useMemo(() => {
    const first = getUtcPeriodRange('monthly', -AVERAGE_MONTHS);
    const last = getUtcPeriodRange('monthly', -1);
    return { start: first.start, end: last.end };
  }, []);
  const { data: averageReport } = useQuery({
    queryKey: ['budgeting-report', averageRange.start, averageRange.end, 'monthly'],
    queryFn: () => budgetingApi.getReport(averageRange.start, averageRange.end, 'monthly'),
    enabled: open,
    staleTime: 5 * 60 * 1000,
  });
  const averages = useMemo(() => {
    const map = new Map<string, number>();
    for (const c of averageReport?.expenseCategories ?? []) {
      map.set(c.categoryId, Math.max(0, parseFloat(c.totalAmount) || 0) / AVERAGE_MONTHS);
    }
    return map;
  }, [averageReport]);

  const selectedCategoryId = form.watch('categoryId');
  const average = selectedCategoryId ? (averages.get(selectedCategoryId) ?? 0) : null;

  const mutation = useMutation({
    mutationFn: (data: InsertBudgetGoal) => budgetingApi.upsertBudgetGoal(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['budgeting-report'] });
      queryClient.invalidateQueries({ queryKey: ['budget-goals'] });
      toast.success(editing ? t('dialog.updated') : t('dialog.added'));
      onOpenChange(false);
    },
    onError: (error: Error) => toast.error(error.message),
  });

  const onSubmit = (values: FormValues) => {
    mutation.mutate({
      categoryId: values.categoryId,
      timeframe: 'monthly',
      amount: String(values.amount),
      // Typed in the display currency: store it with that currency.
      currency: currencyCode,
    });
  };

  const editedCategory = goal ? categories.find((c) => c.id === goal.categoryId) : undefined;
  const editedName = editedCategory ? categoryName(editedCategory) : (goal?.categoryId ?? '');

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{editing ? t('dialog.editTitle') : t('dialog.addTitle')}</DialogTitle>
          <DialogDescription>
            {editing
              ? t('dialog.editDescription', { category: editedName })
              : t('dialog.addDescription')}
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            {!editing && (
              <FormField
                control={form.control}
                name="categoryId"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('dialog.category')}</FormLabel>
                    <Select value={field.value} onValueChange={field.onChange}>
                      <FormControl>
                        <SelectTrigger>
                          <SelectValue placeholder={t('dialog.selectCategory')} />
                        </SelectTrigger>
                      </FormControl>
                      <SelectContent>
                        {availableCategories.map((c) => (
                          <SelectItem key={c.id} value={c.id}>
                            {categoryName(c)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <FormMessage />
                  </FormItem>
                )}
              />
            )}
            <FormField
              control={form.control}
              name="amount"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('dialog.monthlyLimit')}</FormLabel>
                  <FormControl>
                    <InputWrap unit={currencyCode}>
                      <Input
                        type="number"
                        inputMode="decimal"
                        min="0"
                        step="any"
                        placeholder="0"
                        autoFocus={editing}
                        {...field}
                        value={field.value ?? ''}
                      />
                    </InputWrap>
                  </FormControl>
                  <FormDescription>
                    {average === null
                      ? t('dialog.hintPickCategory')
                      : average > 0
                        ? t('dialog.averageHint', { amount: formatCurrency(average) })
                        : t('dialog.noAverage')}
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
                {tc('buttons.cancel')}
              </Button>
              <Button type="submit" loading={mutation.isPending}>
                {editing ? t('dialog.saveLimit') : t('dialog.addTitle')}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
