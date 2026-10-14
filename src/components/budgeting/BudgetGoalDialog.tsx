import { ReportError, ReportQualityNote } from '@/components/reports/ReportQualityNote';
import { currencyDecimals, type CurrencyCode } from '@shared/currencies';
import { preserveMoneyText } from '@/utils/money-flows';
import { formatReportMoney } from '@/utils/report-currencies';
import { useEffect, useMemo, useState } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { TransactionCategory } from '@shared/schema';
import { budgetingApi, type BudgetGoal } from '@/lib/tauri-api';
import { useSaveBudgetGoal } from '@/hooks/use-budget-goal-mutations';
import { requiredNumber } from '@/utils/form-schemas';
import { getUtcPeriodRange } from '@/utils/period';
import {
  currentMonthStart,
  budgetEditSource,
  intervalLastMonth,
  isUnchangedGoalEdit,
  overwrittenPeriods,
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

const formSchema = z
  .object({
    effectiveMonth: z
      .string()
      .refine((value) => monthStartFromInput(value) != null, 'validation.dateRequired'),
    // Empty: the limit runs on.
    effectiveUntil: z
      .string()
      .refine(
        (value) => value === '' || monthStartFromInput(value) != null,
        'validation.dateRequired'
      ),
    categoryId: z.string().min(1, 'validation.categoryIdRequired'),
    amount: requiredNumber('validation.amountRequired').positive('validation.amountPositive'),
  })
  .refine(
    (values) =>
      values.effectiveUntil === '' ||
      (monthStartFromInput(values.effectiveUntil) ?? 0) >=
        (monthStartFromInput(values.effectiveMonth) ?? 0),
    { message: 'validation.effectiveUntilBeforeStart', path: ['effectiveUntil'] }
  );

type FormValues = z.infer<typeof formSchema>;

export interface BudgetGoalDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Editing: the category is fixed and the current limit is prefilled. */
  goal?: BudgetGoal | null;
  /** Adding from an unbudgeted row: that category is preselected. */
  initialCategoryId?: string | null;
  /** Explicit interval selected in settings, or the month shown by the report. */
  initialEffectiveMonth?: number;
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
  initialEffectiveMonth,
  availableCategories,
  categories,
}: BudgetGoalDialogProps) {
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  const { currencyCode, fxRevision } = useCurrency();
  const categoryName = useCategoryName();
  const fmt = useFormat();
  const editing = !!goal;
  const nowMonth = currentMonthStart();
  const monthLabel = (month: number) => fmt.monthInSentence(new Date(month * 1000));

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      categoryId: '',
      amount: 0,
      effectiveMonth: monthInputValue(currentMonthStart()),
      effectiveUntil: '',
    },
  });

  // Every stored limit of the category, so a save can say what it replaces
  // (a new budget may still collide with limits that have run out).
  const { data: history = [] } = useQuery({
    queryKey: ['budget-goal-history'],
    queryFn: () => budgetingApi.getBudgetGoalHistory(),
    enabled: open,
  });
  const selectedCategoryId = form.watch('categoryId');
  const timeframe = goal?.timeframe ?? 'monthly';
  const versions = useMemo(
    () =>
      history.find(
        (g) =>
          g.categoryId === (goal?.categoryId ?? selectedCategoryId) && g.timeframe === timeframe
      )?.versions ??
      goal?.versions ??
      [],
    [history, goal, selectedCategoryId, timeframe]
  );
  // The first click on save lists what the save replaces; the second saves.
  const [confirming, setConfirming] = useState(false);
  useEffect(() => {
    const subscription = form.watch(() => setConfirming(false));
    return () => subscription.unsubscribe();
  }, [form]);
  useEffect(() => {
    if (open) setConfirming(false);
  }, [open]);

  // Reset for each opening: the limit in effect in the edited month (its
  // stored currency), or the preselected category with an empty amount.
  useEffect(() => {
    if (!open) return;
    const now = currentMonthStart();
    if (goal) {
      const source = budgetEditSource(goal, initialEffectiveMonth, now);
      // The edited interval keeps its own end unless the user changes it.
      const last = intervalLastMonth(goal.versions, source.month);
      form.reset({
        categoryId: goal.categoryId,
        amount: Number(source.amount),
        effectiveMonth: monthInputValue(source.month),
        effectiveUntil: last == null ? '' : monthInputValue(last),
      });
    } else {
      form.reset({
        categoryId: initialCategoryId ?? '',
        amount: undefined,
        effectiveMonth: monthInputValue(initialEffectiveMonth ?? now),
        effectiveUntil: '',
      });
    }
  }, [open, goal, initialCategoryId, initialEffectiveMonth, currencyCode, form]);

  // The date can move independently of the selected limit's value.
  const editedMonthValue = form.watch('effectiveMonth');
  const editedMonth = monthStartFromInput(editedMonthValue);
  const editedUntil = monthStartFromInput(form.watch('effectiveUntil'));
  // Moving the effective date moves the selected amount/currency. Taking them
  // from the destination month would silently replace the interval being edited.
  const editSource = goal ? budgetEditSource(goal, initialEffectiveMonth, nowMonth) : null;
  const editorCurrency = editSource?.currency ?? currencyCode;
  const enteredAmount = Number(form.watch('amount'));
  // The save replaces everything in its range; without an end nothing later stays.
  const scheduled = editedUntil == null ? [] : scheduledVersions(versions, editedUntil);

  const editedInterval = useMemo(() => {
    if (!editSource) return null;
    const inEffect = versionInEffect(versions, editSource.month);
    return inEffect
      ? { start: inEffect.effectiveMonth, end: intervalLastMonth(versions, editSource.month) }
      : null;
  }, [editSource, versions]);
  const collisions =
    editedMonth != null && Number.isFinite(enteredAmount) && enteredAmount > 0
      ? overwrittenPeriods(
          versions,
          editedMonth,
          editedUntil,
          enteredAmount,
          editorCurrency,
          editedInterval
        )
      : [];
  const periodLabel = (start: number, end: number | null) => {
    if (end == null) return t('settings.from', { month: monthLabel(start) });
    const first = monthLabel(start);
    const last = monthLabel(end);
    return first === last ? first : t('settings.between', { start: first, end: last });
  };

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
      fxRevision,
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

  const average = selectedCategoryId ? (averages.get(selectedCategoryId) ?? 0) : null;

  const mutation = useSaveBudgetGoal(() => onOpenChange(false), editing);

  const onSubmit = (values: FormValues) => {
    const month = monthStartFromInput(values.effectiveMonth);
    if (month == null) return;
    const until = monthStartFromInput(values.effectiveUntil);
    const currency = editorCurrency;
    // Saving the limit already in effect for that month changes nothing.
    if (goal && isUnchangedGoalEdit(versions, month, values.amount, currency, until)) {
      toast.info(t('readiness.noChange'));
      onOpenChange(false);
      return;
    }
    if (collisions.length > 0 && !confirming) {
      setConfirming(true);
      return;
    }
    const original = goal ? versionInEffect(versions, month) : null;
    mutation.mutate({
      categoryId: values.categoryId,
      timeframe,
      amount: preserveMoneyText(
        editSource?.amount ?? original?.amount ?? null,
        editSource?.currency ?? original?.currency ?? null,
        values.amount,
        currency
      ),
      effectiveMonth: month,
      effectiveUntil: until ?? undefined,
      // Keep the selected interval's own currency; new goals use the profile currency.
      currency,
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
                  <FormLabel>
                    {t('budgetAmount')} · {t(goal?.timeframe ?? 'monthly')}
                  </FormLabel>
                  <FormControl>
                    <InputWrap unit={editorCurrency}>
                      <Input
                        type="number"
                        inputMode="decimal"
                        min="0"
                        step={
                          editSource && Number(editSource.amount) === Number(field.value)
                            ? 'any'
                            : 10 ** -currencyDecimals(editorCurrency as CurrencyCode)
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
                            amount: formatReportMoney(
                              fmt,
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
            <div className="grid grid-cols-2 gap-3">
              <FormField
                control={form.control}
                name="effectiveMonth"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('readiness.effectiveMonth')}</FormLabel>
                    <FormControl>
                      <Input type="month" {...field} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="effectiveUntil"
                render={({ field }) => (
                  <FormItem>
                    <div className="flex items-baseline justify-between gap-2">
                      <FormLabel>{t('readiness.effectiveUntil')}</FormLabel>
                      {field.value !== '' && (
                        <button
                          type="button"
                          onClick={() => field.onChange('')}
                          className="text-micro font-650 text-ink-3 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:underline"
                        >
                          {t('readiness.openEnded')}
                        </button>
                      )}
                    </div>
                    <FormControl>
                      <Input type="month" {...field} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>
            <p className="-mt-2 text-micro text-ink-3">
              {t(editedUntil == null ? 'readiness.rangeOpenHelp' : 'readiness.rangeClosedHelp')}
            </p>
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
                            amount: formatReportMoney(
                              fmt,
                              Number(version.amount),
                              version.currency
                            ),
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
            {confirming && collisions.length > 0 && (
              <div
                role="alert"
                className="rounded-r2 border border-loss-line bg-loss-soft px-3 py-2.5 text-micro text-loss"
              >
                <p className="mb-1 font-650">{t('readiness.overwriteTitle')}</p>
                <ul className="space-y-0.5">
                  {collisions.map(({ version, end }) => (
                    <li key={version.effectiveMonth}>
                      {periodLabel(
                        version.effectiveMonth,
                        end == null ? null : intervalLastMonth(versions, version.effectiveMonth)
                      )}
                      {': '}
                      {formatReportMoney(fmt, Number(version.amount), version.currency)}
                    </li>
                  ))}
                </ul>
              </div>
            )}
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
                {tc('buttons.cancel')}
              </Button>
              <Button type="submit" loading={mutation.isPending}>
                {confirming && collisions.length > 0
                  ? t('readiness.overwriteConfirm')
                  : editing
                    ? t('dialog.saveLimit')
                    : t('dialog.addTitle')}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
