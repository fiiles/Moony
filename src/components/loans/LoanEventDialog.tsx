import { useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
import type { Loan, LoanEventKind } from '@shared/schema';
import { outstandingBalanceAt, type LoanTerms } from '@shared/calculations/loan-amortization';
import { isoDateFromUtcTimestamp } from '@/utils/period';
import { extraPaymentWhatIf, loanTrajectory, type ExtraPaymentMode } from '@/utils/loan-trajectory';
import { useFormat } from '@/lib/use-format';
import { useLoanText } from '@/hooks/use-loan-text';
import { useLoanEventMutations } from '@/hooks/use-loan-events';
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
import { Segmented } from '@/components/ui/segmented';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';

const formSchema = z.object({
  amount: z.string().optional(),
  rate: z.string().optional(),
  payment: z.string().optional(),
  date: z.string().min(1, 'validation.invalidDate'),
  mode: z.enum(['term', 'payment']),
  note: z.string().optional(),
});
type FormValues = z.infer<typeof formSchema>;

export interface LoanEventDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  loan: Loan;
  terms: LoanTerms;
  today: number;
  kind: LoanEventKind;
  /** Prefill from the what-if card. */
  initial?: { amount?: number; mode?: ExtraPaymentMode };
}

const isoToUtcDaySec = (iso: string) => {
  const [y, m, d] = iso.split('-').map(Number);
  return Math.floor(Date.UTC(y, m - 1, d) / 1000);
};
const num = (text: string | undefined) => Number((text ?? '').replace(',', '.')) || 0;

/**
 * One dialog for the three loan events (prototype loan-detail.html): an extra
 * payment with the bank's use of it, a new rate from a day, or the balance from
 * a statement. The alert previews the effect before anything is written.
 */
export function LoanEventDialog({
  open,
  onOpenChange,
  loan,
  terms,
  today,
  kind,
  initial,
}: LoanEventDialogProps) {
  const { t } = useTranslation('loans');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const text = useLoanText();
  const { add } = useLoanEventMutations(loan.id);
  const currency = loan.currency || 'CZK';
  const money = (v: number) => fmt.money(v, currency, { decimals: 0 });
  const balanceToday = Math.max(0, Number(loan.outstandingBalance) || 0);

  const form = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      amount: '',
      rate: '',
      payment: '',
      date: isoDateFromUtcTimestamp(today),
      mode: 'term',
      note: '',
    },
  });

  useEffect(() => {
    if (!open) return;
    form.reset({
      amount:
        kind === 'extra_payment'
          ? initial?.amount !== undefined
            ? String(Math.round(initial.amount))
            : ''
          : kind === 'balance_check'
            ? String(Math.round(balanceToday))
            : '',
      rate: kind === 'rate_change' ? String(Number(loan.interestRate) || 0) : '',
      payment: '',
      date: isoDateFromUtcTimestamp(today),
      mode: initial?.mode ?? 'term',
      note: '',
    });
  }, [open, kind, initial, loan.interestRate, balanceToday, today, form]);

  const amount = num(form.watch('amount'));
  const rate = num(form.watch('rate'));
  const payment = num(form.watch('payment'));
  const mode = form.watch('mode');
  const dateIso = form.watch('date');
  const eventDay = dateIso ? isoToUtcDaySec(dateIso) : today;

  // ---- preview ----
  const whatIf =
    kind === 'extra_payment' && amount > 0 && amount <= balanceToday
      ? extraPaymentWhatIf(terms, today, amount, mode)
      : null;
  const rateChange = (() => {
    if (kind !== 'rate_change') return null;
    const from = (balanceToday * (Number(loan.interestRate) || 0)) / 100 / 12;
    const to = (balanceToday * rate) / 100 / 12;
    const newPayment = payment > 0 ? payment : terms.monthlyPayment;
    const after = loanTrajectory(
      {
        ...terms,
        annualRatePct: rate,
        monthlyPayment: newPayment,
        anchorAmount: balanceToday,
        anchorDay: today,
        endDay: null,
      },
      today
    );
    return { from, to, newPayment, payoffDay: after.payoffDay };
  })();
  const balanceCheck =
    kind === 'balance_check'
      ? {
          computed: outstandingBalanceAt(terms, eventDay),
          delta: amount - outstandingBalanceAt(terms, eventDay),
        }
      : null;

  const tooMuch = kind === 'extra_payment' && amount > balanceToday;
  const canSubmit =
    kind === 'extra_payment'
      ? amount > 0 && !tooMuch
      : kind === 'rate_change'
        ? rate >= 0 && rate <= 100 && (form.watch('rate') ?? '') !== ''
        : amount >= 0 && (form.watch('amount') ?? '') !== '';

  const submit = (values: FormValues) => {
    const base = {
      loanId: loan.id,
      kind,
      eventDay: isoToUtcDaySec(values.date),
      note: values.note?.trim() || null,
    };
    const data =
      kind === 'extra_payment'
        ? {
            ...base,
            amount: String(amount),
            monthlyPayment: values.mode === 'payment' && whatIf ? whatIf.payment.toFixed(2) : null,
          }
        : kind === 'rate_change'
          ? { ...base, rate: String(rate), monthlyPayment: payment > 0 ? String(payment) : null }
          : { ...base, amount: String(amount) };
    add.mutate(data, { onSuccess: () => onOpenChange(false) });
  };

  const k =
    kind === 'extra_payment'
      ? 'extraPayment'
      : kind === 'rate_change'
        ? 'rateChange'
        : 'balanceCheck';
  const minIso = isoDateFromUtcTimestamp(terms.startDay);
  const maxIso = isoDateFromUtcTimestamp(today);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{t(`eventDialog.${k}.title`)}</DialogTitle>
          <DialogDescription>{t(`eventDialog.${k}.description`)}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form id="loan-event-form" onSubmit={form.handleSubmit(submit)} className="space-y-4">
            <div className="grid grid-cols-[1fr_150px] gap-3">
              {kind === 'rate_change' ? (
                <FormField
                  control={form.control}
                  name="rate"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t('eventDialog.rateChange.rate')}</FormLabel>
                      <InputWrap unit="%">
                        <FormControl>
                          <Input type="number" step="0.01" min="0" max="100" autoFocus {...field} />
                        </FormControl>
                      </InputWrap>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              ) : (
                <FormField
                  control={form.control}
                  name="amount"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>{t(`eventDialog.${k}.amount`)}</FormLabel>
                      <InputWrap unit={currency}>
                        <FormControl>
                          <Input type="number" step="0.01" min="0" autoFocus {...field} />
                        </FormControl>
                      </InputWrap>
                      <FormMessage />
                    </FormItem>
                  )}
                />
              )}
              <FormField
                control={form.control}
                name="date"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t(`eventDialog.${k}.date`)}</FormLabel>
                    <FormControl>
                      <Input type="date" min={minIso} max={maxIso} {...field} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>

            {kind === 'extra_payment' && (
              <FormField
                control={form.control}
                name="mode"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>{t('eventDialog.extraPayment.usedFor')}</FormLabel>
                    <Segmented
                      fullWidth
                      value={field.value}
                      onValueChange={field.onChange}
                      options={[
                        { value: 'term', label: t('eventDialog.extraPayment.term') },
                        { value: 'payment', label: t('eventDialog.extraPayment.payment') },
                      ]}
                    />
                  </FormItem>
                )}
              />
            )}

            {kind === 'rate_change' && (
              <FormField
                control={form.control}
                name="payment"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel className="flex justify-between">
                      <span>{t('eventDialog.rateChange.payment')}</span>
                      <span className="font-500 text-ink-5">{tc('labels.optional')}</span>
                    </FormLabel>
                    <InputWrap unit={currency}>
                      <FormControl>
                        <Input type="number" step="0.01" min="0" {...field} />
                      </FormControl>
                    </InputWrap>
                    <FormDescription>
                      {t('eventDialog.rateChange.paymentHint', {
                        amount: money(terms.monthlyPayment),
                      })}
                    </FormDescription>
                    <FormMessage />
                  </FormItem>
                )}
              />
            )}

            <FormField
              control={form.control}
              name="note"
              render={({ field }) => (
                <FormItem>
                  <FormLabel className="flex justify-between">
                    <span>{t('eventDialog.note')}</span>
                    <span className="font-500 text-ink-5">{tc('labels.optional')}</span>
                  </FormLabel>
                  <FormControl>
                    <Input {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />

            <Alert>
              <Info aria-hidden />
              {kind === 'extra_payment' &&
                (tooMuch ? (
                  <AlertTitle>
                    {t('eventDialog.extraPayment.alertTooMuch', { amount: money(balanceToday) })}
                  </AlertTitle>
                ) : whatIf ? (
                  <>
                    <AlertTitle>
                      {t('eventDialog.extraPayment.alertTitle', {
                        amount: money(whatIf.balanceAfter),
                      })}
                    </AlertTitle>
                    <AlertDescription>
                      {mode === 'term'
                        ? t('eventDialog.extraPayment.alertTerm', {
                            duration: text.months(whatIf.monthsBefore - whatIf.monthsAfter),
                            interest: money(whatIf.interestBefore - whatIf.interestAfter),
                          })
                        : t('eventDialog.extraPayment.alertPayment', {
                            payment: money(whatIf.payment),
                            month:
                              whatIf.payoffDay !== null ? text.monthYear(whatIf.payoffDay) : '—',
                          })}
                    </AlertDescription>
                  </>
                ) : (
                  <AlertTitle>{t('eventDialog.extraPayment.alertEmpty')}</AlertTitle>
                ))}
              {kind === 'rate_change' && rateChange && (
                <>
                  <AlertTitle>
                    {t('eventDialog.rateChange.alertTitle', {
                      from: money(rateChange.from),
                      to: money(rateChange.to),
                    })}
                  </AlertTitle>
                  <AlertDescription>
                    {rateChange.payoffDay !== null
                      ? t('eventDialog.rateChange.alertNote', {
                          payment: money(rateChange.newPayment),
                          month: text.monthYear(rateChange.payoffDay),
                        })
                      : t('eventDialog.rateChange.alertNoPayoff', {
                          payment: money(rateChange.newPayment),
                        })}
                  </AlertDescription>
                </>
              )}
              {kind === 'balance_check' && balanceCheck && (
                <>
                  <AlertTitle>
                    {t('eventDialog.balanceCheck.alertTitle', {
                      amount: money(balanceCheck.computed),
                    })}
                  </AlertTitle>
                  <AlertDescription>
                    {t('eventDialog.balanceCheck.alertNote', {
                      delta: fmt.money(balanceCheck.delta, currency, { signed: true, decimals: 0 }),
                      date: fmt.day(eventDay),
                    })}
                  </AlertDescription>
                </>
              )}
            </Alert>
          </form>
        </Form>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form="loan-event-form"
            disabled={add.isPending || !canSubmit}
            loading={add.isPending}
          >
            {add.isPending ? tc('status.saving') : t(`eventDialog.${k}.submit`)}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
