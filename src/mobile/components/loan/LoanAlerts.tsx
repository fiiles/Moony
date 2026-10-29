import { useTranslation } from 'react-i18next';
import { CalendarClock, Info, TriangleAlert } from 'lucide-react';
import type { LoanSchedule } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import type { LoanRow } from '@/utils/loans';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { loanAlerts, manualBalance, type LoanAlert } from '@/mobile/lib/loan-detail';

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
 * note.
 */
export function LoanAlerts({ row, schedule, today, onRecordRate }: LoanAlertsProps) {
  const { t } = useTranslation('mobile');
  const { t: tl } = useTranslation('loans');
  const fmt = useFormat();
  const moneyText = useMoneyText();
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

  // The desktop's valuation note, worded here: its hook words the amount its own way
  const manual = manualBalance(row, today);

  return (
    <>
      {loanAlerts(row, schedule, today).map(alert)}
      {manual && (
        <InlineAlert icon={Info} muted>
          {tl('valuation.manual', { amount: money(manual.amount), date: fmt.day(manual.day) })}
        </InlineAlert>
      )}
    </>
  );
}
