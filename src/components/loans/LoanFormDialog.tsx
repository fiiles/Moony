import { useEffect } from 'react';
import { useForm, useWatch, type DefaultValues } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useTranslation } from 'react-i18next';
import { insertLoanSchema, type InsertLoan, type Loan } from '@shared/schema';
import {
  dayFloor,
  loanValuationMode,
  outstandingBalanceAt,
  todayUtcDay,
  type LoanTerms,
} from '@shared/calculations/loan-amortization';
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
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { FormSection } from '@/components/ui/form-section';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { isoDateFromUtcTimestamp } from '@/utils/period';
import { CurrencyCombobox } from '@/components/common/CurrencyCombobox';

interface LoanFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: InsertLoan | (Partial<Loan> & { id: string })) => void;
  loan?: Loan | null;
  isLoading?: boolean;
}

/** The shared schema plus the UI-only switch "set the balance as of a date". */
const loanFormSchema = insertLoanSchema.extend({ manualBalance: z.boolean() });
type LoanFormValues = z.infer<typeof loanFormSchema>;

type DayValue = Date | number | null | undefined;

/**
 * Day fields are calendar days stored as UTC midnight (ADR 0008): the <input type="date">
 * value is shown and parsed in UTC, so a user west of UTC no longer sees yesterday.
 */
function toSeconds(value: DayValue): number | undefined {
  if (value instanceof Date) return Math.floor(value.getTime() / 1000);
  if (typeof value === 'number') return value;
  return undefined;
}

function toDayInput(value: DayValue): string {
  const seconds = toSeconds(value);
  return seconds === undefined ? '' : isoDateFromUtcTimestamp(seconds);
}

/** `new Date('YYYY-MM-DD')` is the UTC midnight of that day. */
function fromDayInput(iso: string): Date | undefined {
  return iso ? new Date(iso) : undefined;
}

function toDay(value: DayValue): number | undefined {
  const seconds = toSeconds(value);
  return seconds === undefined ? undefined : dayFloor(seconds);
}

function emptyDefaults(currency: string): DefaultValues<LoanFormValues> {
  return {
    name: '',
    principal: '',
    currency,
    interestRate: '',
    monthlyPayment: '',
    interestRateValidityDate: undefined,
    startDate: new Date(todayUtcDay() * 1000),
    endDate: undefined,
    balanceAnchorAmount: '',
    balanceAnchorDate: undefined,
    manualBalance: false,
  } as DefaultValues<LoanFormValues>;
}

/**
 * Loan form (design system §6 Modal): principal and currency, rate with its
 * fixation end, payment, start and end, and the balance section with the live
 * estimate and the "balance as of a date" switch.
 */
export function LoanFormDialog({
  open,
  onOpenChange,
  onSubmit,
  loan,
  isLoading,
}: LoanFormDialogProps) {
  const { t } = useTranslation('loans');
  const { t: tc } = useTranslation('common');
  const { currencyCode: userCurrency } = useCurrency();
  const fmt = useFormat();

  const form = useForm<LoanFormValues>({
    resolver: zodResolver(loanFormSchema),
    defaultValues: emptyDefaults(userCurrency),
  });

  const watched = useWatch({ control: form.control });
  // "Set the balance as of a date": off = amortize from the principal at the start date
  const manualBalance = !!watched.manualBalance;
  const currency = watched.currency || 'CZK';

  useEffect(() => {
    if (!open) return;
    if (loan) {
      // Display loan in its STORED currency (no conversion)
      form.reset({
        name: loan.name,
        principal: loan.principal.toString(),
        currency: loan.currency || 'CZK',
        interestRate: loan.interestRate.toString(),
        monthlyPayment: loan.monthlyPayment?.toString() || '0',
        interestRateValidityDate: loan.interestRateValidityDate
          ? new Date(loan.interestRateValidityDate * 1000)
          : undefined,
        startDate: loan.startDate ? new Date(loan.startDate * 1000) : new Date(),
        endDate: loan.endDate ? new Date(loan.endDate * 1000) : undefined,
        balanceAnchorAmount: loan.balanceAnchorAmount ?? '',
        balanceAnchorDate: loan.balanceAnchorDate
          ? new Date(loan.balanceAnchorDate * 1000)
          : undefined,
        manualBalance: typeof loan.balanceAnchorAmount === 'string',
      } as DefaultValues<LoanFormValues>);
    } else {
      form.reset(emptyDefaults(userCurrency));
    }
  }, [loan, form, open, userCurrency]);

  // Live estimate of today's balance from what is typed (same maths as the backend)
  const estimate = (() => {
    const principal = Number(watched.principal);
    const startSeconds = toSeconds(watched.startDate);
    if (!(principal > 0) || startSeconds === undefined) return null;
    const anchorAmount = (watched.balanceAnchorAmount ?? '').trim();
    const anchorSeconds = toSeconds(watched.balanceAnchorDate);
    const useAnchor = manualBalance && anchorAmount !== '' && anchorSeconds !== undefined;
    const terms: LoanTerms = {
      principal,
      annualRatePct: Number(watched.interestRate) || 0,
      monthlyPayment: Number(watched.monthlyPayment) || 0,
      startDay: dayFloor(startSeconds),
      anchorAmount: useAnchor ? Number(anchorAmount) || 0 : null,
      anchorDay: useAnchor ? dayFloor(anchorSeconds) : null,
    };
    return {
      value: outstandingBalanceAt(terms, todayUtcDay()),
      withoutPayment: loanValuationMode(terms) === 'static',
    };
  })();

  const handleManualBalanceChange = (checked: boolean) => {
    form.setValue('manualBalance', checked);
    if (!checked) {
      form.clearErrors(['balanceAnchorAmount', 'balanceAnchorDate']);
      return;
    }
    // Start from today's estimate so the user only corrects it
    if (!(form.getValues('balanceAnchorAmount') ?? '').trim() && estimate) {
      form.setValue('balanceAnchorAmount', estimate.value.toFixed(2));
    }
    if (!form.getValues('balanceAnchorDate')) {
      form.setValue('balanceAnchorDate', new Date(todayUtcDay() * 1000));
    }
  };

  const handleSubmit = ({ manualBalance: manual, ...data }: LoanFormValues) => {
    let balanceAnchorAmount: string | null = null;
    let balanceAnchorDate: number | null = null;
    if (manual) {
      const amount = (data.balanceAnchorAmount ?? '').trim();
      const day = toDay(data.balanceAnchorDate);
      if (!amount) {
        form.setError('balanceAnchorAmount', {
          type: 'manual',
          message: 'validation.balanceAnchorIncomplete',
        });
      }
      if (day === undefined) {
        form.setError('balanceAnchorDate', {
          type: 'manual',
          message: 'validation.balanceAnchorIncomplete',
        });
      }
      if (!amount || day === undefined) return;
      balanceAnchorAmount = amount;
      balanceAnchorDate = day;
    }

    // Send amounts in ORIGINAL currency (no conversion to CZK); days as UTC midnight
    const submissionData = {
      ...data,
      principal: data.principal,
      monthlyPayment: data.monthlyPayment || '0',
      startDate: toDay(data.startDate),
      endDate: toDay(data.endDate),
      interestRateValidityDate: toDay(data.interestRateValidityDate),
      balanceAnchorAmount,
      balanceAnchorDate,
    };

    if (loan) {
      onSubmit({ ...submissionData, id: loan.id });
    } else {
      onSubmit(submissionData as InsertLoan);
    }
  };

  const dateField = (
    name: 'startDate' | 'endDate' | 'interestRateValidityDate',
    label: string,
    hint?: string,
    optional = false
  ) => (
    <FormField
      control={form.control}
      name={name}
      render={({ field }) => (
        <FormItem>
          <FormLabel className="flex justify-between">
            <span>{label}</span>
            {optional && <span className="font-500 text-ink-5">{tc('labels.optional')}</span>}
          </FormLabel>
          <FormControl>
            <Input
              type="date"
              value={toDayInput(field.value as DayValue)}
              onChange={(e) => field.onChange(fromDayInput(e.target.value))}
            />
          </FormControl>
          {hint && <FormDescription>{hint}</FormDescription>}
          <FormMessage />
        </FormItem>
      )}
    />
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{loan ? t('form.editTitle') : t('form.addTitle')}</DialogTitle>
          <DialogDescription>
            {loan ? t('modal.editDescription') : t('modal.addDescription')}
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form id="loan-form" onSubmit={form.handleSubmit(handleSubmit)} className="space-y-4">
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

            <div className="grid grid-cols-[1fr_138px] gap-3">
              <FormField
                control={form.control}
                name="principal"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('form.originalAmount')}</FormLabel>
                    <InputWrap unit={currency}>
                      <FormControl>
                        <Input type="number" step="0.01" min="0" placeholder="0" {...field} />
                      </FormControl>
                    </InputWrap>
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
                name="interestRate"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('form.interestRate')}</FormLabel>
                    <InputWrap unit="%">
                      <FormControl>
                        <Input type="number" step="0.01" min="0" placeholder="0" {...field} />
                      </FormControl>
                    </InputWrap>
                    <FormMessage />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="monthlyPayment"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('form.monthlyPayment')}</FormLabel>
                    <InputWrap unit={currency}>
                      <FormControl>
                        <Input type="number" step="0.01" min="0" placeholder="0" {...field} />
                      </FormControl>
                    </InputWrap>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>

            <div className="grid grid-cols-2 gap-3">
              {dateField('startDate', t('form.startDate'), t('modal.startDateHelp'))}
              {dateField('endDate', t('form.endDate'), t('modal.endDateHelp'), true)}
            </div>
            <div className="grid grid-cols-2 gap-3">
              {dateField(
                'interestRateValidityDate',
                t('modal.rateValidity'),
                t('modal.rateValidityHelp'),
                true
              )}
            </div>

            <FormSection title={t('form.balance.title')}>
              <p className="text-caption text-ink-3">{t('form.balance.help')}</p>
              {estimate && (
                <div className="rounded-r3 bg-well px-4 py-3 text-table">
                  <p className="font-600 text-ink num" data-testid="loan-balance-estimate">
                    {t('form.balance.estimate', {
                      amount: fmt.money(estimate.value, currency),
                    })}
                  </p>
                  {estimate.withoutPayment && (
                    <p className="mt-0.5 text-caption text-ink-3">{t('form.balance.noPayment')}</p>
                  )}
                </div>
              )}
              <div className="flex items-center justify-between">
                <Label htmlFor="manualBalance">{t('form.balance.setManual')}</Label>
                <Switch
                  id="manualBalance"
                  checked={manualBalance}
                  onCheckedChange={handleManualBalanceChange}
                />
              </div>
              {manualBalance && (
                <div className="grid grid-cols-2 gap-3">
                  <FormField
                    control={form.control}
                    name="balanceAnchorAmount"
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{t('form.balance.amount')}</FormLabel>
                        <InputWrap unit={currency}>
                          <FormControl>
                            <Input
                              type="number"
                              step="0.01"
                              min="0"
                              placeholder="0"
                              {...field}
                              value={field.value ?? ''}
                            />
                          </FormControl>
                        </InputWrap>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  {dateField('balanceAnchorDate' as 'startDate', t('form.balance.date'))}
                </div>
              )}
            </FormSection>
          </form>
        </Form>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button type="submit" form="loan-form" disabled={isLoading} loading={isLoading}>
            {isLoading ? tc('status.saving') : loan ? tc('buttons.saveChanges') : t('form.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
