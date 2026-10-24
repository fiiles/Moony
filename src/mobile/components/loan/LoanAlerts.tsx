import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { CalendarClock, Info, TriangleAlert, type LucideIcon } from 'lucide-react';
import type { LoanSchedule } from '@shared/schema';
import { dayFloor } from '@shared/calculations/loan-amortization';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import type { LoanRow } from '@/utils/loans';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { loanAlerts, type LoanAlert } from '@/mobile/lib/loan-detail';

interface InlineAlertProps {
  icon: LucideIcon;
  /** The quiet variant: no border (the manual balance's note, which asks for nothing). */
  muted?: boolean;
  /** A text action under the sentence: a 44 pt target. */
  action?: { label: string; onClick: () => void };
  children: ReactNode;
}

/**
 * An inline alert (README §13, `.m-alert`): a sentence next to its cause on `well`, radius 11, 12 /
 * 500 `ink-2`, a 15 pt glyph in `ink-3`, at least 44 pt tall, wrapping. It may end with one text
 * action. A warning has the hairline border (the desktop's alert), a note has none.
 */
export function InlineAlert({ icon: Icon, muted = false, action, children }: InlineAlertProps) {
  return (
    <div
      role={muted ? 'note' : 'alert'}
      className={cn(
        'mb-2.5 flex min-h-11 items-start gap-2.5 rounded-[11px] border bg-well px-3 py-2.5 text-m-sub text-ink-2',
        muted ? 'border-transparent' : 'border-line'
      )}
    >
      <Icon aria-hidden className="mt-px size-[15px] flex-none text-ink-3" strokeWidth={1.75} />
      <div className="min-w-0 leading-[1.4]">
        <p className="m-0">{children}</p>
        {action && (
          <button
            type="button"
            onClick={action.onClick}
            className="-mb-2.5 mt-0.5 inline-flex min-h-11 items-center text-m-sub font-semibold text-ink [-webkit-touch-callout:none] active:text-ink-3"
          >
            {action.label}
          </button>
        )}
      </div>
    </div>
  );
}

interface LoanAlertsProps {
  row: LoanRow;
  schedule: LoanSchedule;
  today: number;
  /** "Zapsat změnu sazby": opens the rate-change dialog. */
  onRecordRate: () => void;
}

/**
 * The warnings under the loan's hero (spec §8.8): no payment, a payment below the interest, a
 * balance left at the end date, a cut-off schedule (the desktop's `LoanWarnings`, its strings), and
 * the fixation that ends within 30 days or ended up to 60 days ago, each fixation alert with the
 * action that opens the rate-change dialog. A hand-entered balance says that it holds, in one quiet
 * note. Amounts go through the eye.
 */
export function LoanAlerts({ row, schedule, today, onRecordRate }: LoanAlertsProps) {
  const { t } = useTranslation('mobile');
  const { t: tl } = useTranslation('loans');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { loan } = row;
  const money = (value: number) => moneyText(value, row.currency);
  const recordRate = { label: tl('actions.rateChange'), onClick: onRecordRate };

  const alert = (a: LoanAlert) => {
    switch (a.kind) {
      case 'noPayment':
        return (
          <InlineAlert key={a.kind} icon={TriangleAlert}>
            {tl('detail.warnings.noPayment')}
          </InlineAlert>
        );
      case 'paymentBelowInterest':
        return (
          <InlineAlert key={a.kind} icon={TriangleAlert}>
            {tl('detail.warnings.paymentBelowInterest', { interest: money(a.interest) })}
          </InlineAlert>
        );
      case 'balanceAtEndDate':
        return (
          <InlineAlert key={a.kind} icon={TriangleAlert}>
            {tl('detail.warnings.balanceAtEndDate', {
              date: a.day === null ? '' : fmt.day(a.day),
              amount: money(a.amount),
            })}
          </InlineAlert>
        );
      case 'scheduleTruncated':
        return (
          <InlineAlert key={a.kind} icon={TriangleAlert}>
            {tl('detail.warnings.scheduleTruncated', { count: a.count })}
          </InlineAlert>
        );
      case 'fixationSoon':
        return (
          <InlineAlert key={a.kind} icon={CalendarClock} action={recordRate}>
            {a.days === 0
              ? t('loan.alerts.fixationToday')
              : t('loan.alerts.fixationSoon', { count: a.days })}
          </InlineAlert>
        );
      case 'fixationPassed':
        return (
          <InlineAlert key={a.kind} icon={CalendarClock} action={recordRate}>
            {t('loan.alerts.fixationPassed', { count: a.days })}
          </InlineAlert>
        );
    }
  };

  // The desktop's valuation note, worded here: its hook formats the amount without the eye
  const manual =
    row.mode === 'manual' && loan.balanceAnchorAmount !== null && loan.balanceAnchorDate !== null;

  return (
    <>
      {loanAlerts(row, schedule, today).map(alert)}
      {manual && (
        <InlineAlert icon={Info} muted>
          {tl('valuation.manual', {
            amount: money(Number(loan.balanceAnchorAmount) || 0),
            date: fmt.day(dayFloor(loan.balanceAnchorDate!)),
          })}
        </InlineAlert>
      )}
    </>
  );
}
