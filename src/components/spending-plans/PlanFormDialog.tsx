import { useEffect, useMemo } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { InsertSpendingPlan, SpendingPlan } from '@shared/schema';
import { currencyCodeSchema, type CurrencyCode } from '@shared/currencies';
import { requiredNumber } from '@/utils/form-schemas';
import { useCurrency } from '@/lib/currency';
import { ApiError } from '@/lib/api-error';
import { useSpendingPlanMutations } from '@/hooks/use-spending-plan-mutations';
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
import { Textarea } from '@/components/ui/textarea';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';
import { DEFAULT_COLOR, IconColorPicker } from '@/components/common/IconColorPicker';

const DEFAULT_ICON = 'wallet';
/** Same limits as `InsertSpendingPlan::validate` in the backend. */
const NAME_MAX = 100;
const NOTE_MAX = 500;

const formSchema = z.object({
  name: z
    .string()
    .trim()
    .min(1, 'validation.spendingPlanNameRequired')
    .max(NAME_MAX, 'validation.spendingPlanNameTooLong'),
  amount: requiredNumber('validation.spendingPlanAmountInvalid').positive(
    'validation.spendingPlanAmountInvalid'
  ),
  year: z.string(),
  currency: currencyCodeSchema,
  icon: z.string(),
  color: z.string(),
  note: z.string().max(NOTE_MAX, 'validation.spendingPlanNoteTooLong'),
});
type FormValues = z.infer<typeof formSchema>;

interface PlanFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The plan being edited; omit to create one. */
  plan?: SpendingPlan | null;
  /** Year of a new plan (the page's year). */
  defaultYear: number;
  /** Called with a newly created plan (not after an edit). */
  onCreated?: (plan: SpendingPlan) => void;
}

/**
 * New / edit spending plan (spec §9 "Plan form dialog"): name; planned amount (its unit follows
 * the currency), year and currency; the ECB hint; an optional note; icon, color and a preview of
 * the square plan chip. No delete here. A name already used in the year is shown under the name.
 */
export function PlanFormDialog({
  open,
  onOpenChange,
  plan = null,
  defaultYear,
  onCreated,
}: PlanFormDialogProps) {
  const { t } = useTranslation('spending_plans');
  const { t: tc } = useTranslation('common');
  const { currencyCode } = useCurrency();
  const { createPlan, updatePlan } = useSpendingPlanMutations();
  const isEdit = plan !== null;

  const defaults = useMemo<FormValues>(
    () =>
      plan
        ? {
            name: plan.name,
            amount: parseFloat(plan.amount) || 0,
            year: String(plan.year),
            currency: plan.currency as CurrencyCode,
            icon: plan.icon || DEFAULT_ICON,
            color: plan.color || DEFAULT_COLOR,
            note: plan.note ?? '',
          }
        : {
            name: '',
            amount: undefined as unknown as number,
            year: String(defaultYear),
            currency: currencyCode,
            icon: DEFAULT_ICON,
            color: DEFAULT_COLOR,
            note: '',
          },
    [plan, defaultYear, currencyCode]
  );

  const form = useForm<FormValues>({ resolver: zodResolver(formSchema), defaultValues: defaults });

  useEffect(() => {
    if (open) form.reset(defaults);
  }, [open, defaults, form]);

  // Last year, this year, next year, plus the plan's (or page's) own year when outside them
  const years = useMemo(() => {
    const current = new Date().getFullYear();
    const set = new Set([current - 1, current, current + 1, plan?.year ?? defaultYear]);
    return [...set].sort((a, b) => a - b);
  }, [plan, defaultYear]);

  const currency = form.watch('currency');
  const icon = form.watch('icon');
  const color = form.watch('color');
  const name = form.watch('name');
  const isPending = createPlan.isPending || updatePlan.isPending;

  const submit = async (values: FormValues) => {
    const data: InsertSpendingPlan = {
      name: values.name.trim(),
      year: Number(values.year),
      amount: String(values.amount),
      currency: values.currency,
      icon: values.icon,
      color: values.color,
      note: values.note.trim() ? values.note.trim() : null,
    };
    try {
      if (plan) {
        await updatePlan.mutateAsync({ id: plan.id, data });
        onOpenChange(false);
      } else {
        const created = await createPlan.mutateAsync(data);
        toast.success(t('toast.created', { name: created.name }));
        onOpenChange(false);
        onCreated?.(created);
      }
    } catch (error) {
      // A taken name belongs under the field; every other error was toasted by the mutation.
      if (error instanceof ApiError && error.key === 'validation.spendingPlanNameTaken') {
        form.setError('name', { message: 'validation.spendingPlanNameTaken' });
      }
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[90vh] flex-col">
        <DialogHeader>
          <DialogTitle>{isEdit ? t('form.editTitle') : t('form.newTitle')}</DialogTitle>
          <DialogDescription>
            {plan
              ? t('form.editDescription', { name: plan.name, year: plan.year })
              : t('form.newDescription')}
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(submit)} className="flex min-h-0 flex-1 flex-col gap-4">
            {/* Only the fields scroll; the header and the footer with Vytvořit stay in view */}
            <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-px py-1">
              <FormField
                control={form.control}
                name="name"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('form.name')}</FormLabel>
                    <FormControl>
                      <Input
                        placeholder={t('form.namePlaceholder')}
                        maxLength={NAME_MAX}
                        autoFocus
                        {...field}
                      />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <div className="space-y-2">
                <div className="grid grid-cols-[1fr_96px_120px] gap-3">
                  <FormField
                    control={form.control}
                    name="amount"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('form.amount')}</FormLabel>
                        <InputWrap unit={currency}>
                          <FormControl>
                            <Input
                              type="number"
                              step="any"
                              min="0"
                              placeholder="0"
                              className="num"
                              {...field}
                              value={field.value ?? ''}
                            />
                          </FormControl>
                        </InputWrap>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    control={form.control}
                    name="year"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('form.year')}</FormLabel>
                        <Select value={field.value} onValueChange={field.onChange}>
                          <FormControl>
                            <SelectTrigger>
                              <SelectValue />
                            </SelectTrigger>
                          </FormControl>
                          <SelectContent>
                            {years.map((y) => (
                              <SelectItem key={y} value={String(y)}>
                                {y}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
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
                          <CurrencyCombobox
                            value={field.value}
                            onChange={field.onChange}
                            showName={false}
                          />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                </div>
                <FormDescription>{t('form.fxHint')}</FormDescription>
              </div>
              <FormField
                control={form.control}
                name="note"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel className="flex justify-between">
                      <span>{t('form.note')}</span>
                      <span className="font-500 text-ink-5">{tc('labels.optional')}</span>
                    </FormLabel>
                    <FormControl>
                      <Textarea
                        rows={2}
                        maxLength={NOTE_MAX}
                        placeholder={t('form.notePlaceholder')}
                        {...field}
                      />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <IconColorPicker
                icon={icon}
                color={color}
                onIconChange={(value) => form.setValue('icon', value, { shouldDirty: true })}
                onColorChange={(value) => form.setValue('color', value, { shouldDirty: true })}
                previewLabel={name.trim() || t('form.previewName')}
                previewShape="square"
              />
            </div>
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
                {tc('buttons.cancel')}
              </Button>
              <Button type="submit" disabled={isPending}>
                {isEdit ? t('form.save') : t('form.create')}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
