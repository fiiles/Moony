import { ReportError, ReportQualityNote } from '@/components/reports/ReportQualityNote';
import { currencyDecimals, type CurrencyCode } from '@shared/currencies';
import { preserveMoneyText } from '@/utils/money-flows';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { translateApiError } from '@/lib/translate-api-error';
import { useEffect, useMemo } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { TransactionCategory } from '@shared/schema';
import { budgetingApi, type BudgetGoal, type InsertBudgetGoal } from '@/lib/tauri-api';
import { requiredNumber } from '@/utils/form-schemas';
import { getUtcPeriodRange } from '@/utils/period';
import {
  currentMonthStart,
  defaultEditMonth,
  isUnchangedGoalEdit,
  monthInputValue,
  monthStartFromInput,
  scheduledVersions,
  versionInEffect,
} from '@/utils/budget-goal-versions';
import { useFormat } from '@/lib/use-format';
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
  effectiveMonth: z.string().regex(/^\d{4}-\d{2}$/, 'validation.dateRequired'),
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
  const { currencyCode, ratesTimestamp } = useCurrency();
  const editorCurrency = goal?.currency ?? currencyCode;
  const categoryName = useCategoryName();
  const queryClient = useQueryClient();
  const fmt = useFormat();
  const editing = !!goal;
  const versions = useMemo(() => goal?.versions ?? [], [goal]);
  const nowMonth = currentMonthStart();
  const monthLabel = (month: number) => fmt.month(new Date(month * 1000));

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      categoryId: '',
      amount: 0,
      effectiveMonth: monthInputValue(currentMonthStart()),
    },
  });

  // Reset for each opening: the limit in effect in the edited month (its
  // stored currency), or the preselected category with an empty amount.
  useEffect(() => {
    if (!open) return;
    const now = currentMonthStart();
    if (goal) {
      const month = defaultEditMonth(goal.versions, now);
      const current = versionInEffect(goal.versions, month);
      form.reset({
        categoryId: goal.categoryId,
        amount: Number(current?.amount ?? goal.amount),
        effectiveMonth: monthInputValue(month),
      });
    } else {
      form.reset({
        categoryId: initialCategoryId ?? '',
        amount: undefined,
        effectiveMonth: monthInputValue(now),
      });
    }
  }, [open, goal, initialCategoryId, currencyCode, form]);

  // Another month shows the limit in effect then, unless the user already
  // typed a new amount.
  const editedMonthValue = form.watch('effectiveMonth');
  const editedMonth = monthStartFromInput(editedMonthValue);
  const editedVersion = editedMonth == null ? null : versionInEffect(versions, editedMonth);
  useEffect(() => {
    if (!open || !goal || editedMonth == null) return;
    if (form.getFieldState('amount').isDirty) return;
    const version = versionInEffect(goal.versions, editedMonth);
    if (version?.amount != null) form.setValue('amount', Number(version.amount));
  }, [open, goal, editedMonth, form]);
  const scheduled = scheduledVersions(versions, editedMonth ?? nowMonth);

  // Average monthly spending in the limit editor's own currency.
  const averageRange = useMemo(() => {
    const first = getUtcPeriodRange('monthly', -AVERAGE_MONTHS);
    const last = getUtcPeriodRange('monthly', -1);
    return { start: first.start, end: last.end };
  }, []);
  const {
    data: averageReport,
    isError: averageError,
    refetch: refetchAverage,
  } = useQuery({
    queryKey: [
      'budgeting-report',
      averageRange.start,
      averageRange.end,
      'monthly',
      editorCurrency,
      ratesTimestamp,
    ],
    queryFn: () =>
      budgetingApi.getReport(
        averageRange.start,
        averageRange.end,
        'monthly',
        undefined,
        editorCurrency
      ),
    enabled: open,
    staleTime: 5 * 60 * 1000,
  });
  const averages = useMemo(() => {
    const map = new Map<string, number>();
    if (
      !averageReport ||
      averageReport.quality.missingFxCount > 0 ||
      averageReport.quality.invalidAmountCount > 0 ||
      !averageReport.quality.observedCompletedMonths.length
    )
      return map;
    for (const c of averageReport?.expenseCategories ?? []) {
      map.set(
        c.categoryId,
        (parseFloat(c.totalAmount) || 0) /
          Math.max(1, averageReport?.quality.observedCompletedMonths.length ?? 0)
      );
    }
    return map;
  }, [averageReport]);

  const selectedCategoryId = form.watch('categoryId');
  const average = selectedCategoryId ? (averages.get(selectedCategoryId) ?? 0) : null;

  const mutation = useMutation({
    mutationFn: (data: InsertBudgetGoal) => budgetingApi.upsertBudgetGoal(data),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast.success(editing ? t('dialog.updated') : t('dialog.added'));
      onOpenChange(false);
    },
    onError: (error: Error) => toast.error(translateApiError(error, tc)),
  });

  const onSubmit = (values: FormValues) => {
    const month = monthStartFromInput(values.effectiveMonth);
    if (month == null) return;
    const currency = goal?.currency ?? currencyCode;
    // Saving the limit already in effect for that month changes nothing.
    if (goal && isUnchangedGoalEdit(goal.versions, month, values.amount, currency)) {
      toast.info(t('readiness.noChange'));
      onOpenChange(false);
      return;
    }
    const original = goal ? versionInEffect(goal.versions, month) : null;
    mutation.mutate({
      categoryId: values.categoryId,
      timeframe: goal?.timeframe ?? 'monthly',
      amount: preserveMoneyText(
        original?.amount ?? null,
        original?.currency ?? null,
        values.amount,
        currency
      ),
      effectiveMonth: month,
      // Typed in the display currency: store it with that currency.
      currency: goal?.currency ?? currencyCode,
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
                    <InputWrap unit={goal?.currency ?? currencyCode}>
                      <Input
                        type="number"
                        inputMode="decimal"
                        min="0"
                        step={
                          editedVersion && Number(editedVersion.amount) === Number(field.value)
                            ? 'any'
                            : 10 **
                              -currencyDecimals((goal?.currency ?? currencyCode) as CurrencyCode)
                        }
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
                        ? t('readiness.observedAverage', {
                            amount: fmt.money(
                              average,
                              averageReport?.reportCurrency ?? editorCurrency
                            ),
                            count: averageReport?.quality.observedCompletedMonths.length ?? 0,
                          })
                        : t('dialog.noAverage')}
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="effectiveMonth"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('readiness.effectiveMonth')}</FormLabel>
                  <FormControl>
                    <Input type="month" {...field} />
                  </FormControl>
                  <FormDescription>
                    {t('readiness.effectiveMonthHelp')}
                    {editing && editedMonth != null && (
                      <>
                        {' '}
                        {t('readiness.effectiveMonthPrefill', { month: monthLabel(editedMonth) })}
                      </>
                    )}
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            {scheduled.length > 0 && (
              <div className="rounded-r2 border border-line bg-well px-3 py-2.5 text-micro text-ink-3">
                <p className="mb-1 font-650 text-ink-2">{t('readiness.scheduledHeading')}</p>
                <ul className="space-y-0.5">
                  {scheduled.map((version) => (
                    <li key={version.effectiveMonth}>
                      {version.amount == null
                        ? t('readiness.scheduledRemoval', {
                            month: monthLabel(version.effectiveMonth),
                          })
                        : t('readiness.scheduledLimit', {
                            month: monthLabel(version.effectiveMonth),
                            amount: fmt.money(Number(version.amount), version.currency),
                          })}
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {averageError ? (
              <ReportError onRetry={() => void refetchAverage()} />
            ) : (
              averageReport && <ReportQualityNote quality={averageReport.quality} />
            )}
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
