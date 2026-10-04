import { useTranslation } from 'react-i18next';
import type { LoanMetrics } from '@/utils/loans';
import { monthsAhead } from '@/utils/loans';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useLoanText } from '@/hooks/use-loan-text';
import { Stat, Stats } from '@/components/common/Stat';

interface LoansSummaryProps {
  metrics: LoanMetrics;
  today: number;
}

/**
 * Four stats (prototype loans.html): outstanding with the repaid share, monthly
 * payments and what they drop to after the next payoff, the balance-weighted
 * rate with the interest due in the next year, and the nearest fixation end.
 */
export function LoansSummary({ metrics, today }: LoansSummaryProps) {
  const { t } = useTranslation('loans');
  const { formatCurrency } = useCurrency();
  const fmt = useFormat();
  const text = useLoanText();

  const repaidShare =
    metrics.totalPrincipal > 0
      ? Math.max(0, 1 - metrics.totalOutstanding / metrics.totalPrincipal)
      : 0;

  const paymentsNote = metrics.nextPayoff
    ? t('summary.paymentsAfter', {
        month: text.monthYear(metrics.nextPayoff.day),
        amount: formatCurrency(metrics.nextPayoff.paymentsAfter),
        name: metrics.nextPayoff.row.loan.name,
      })
    : t('summary.paymentsYear', { amount: formatCurrency(metrics.totalMonthlyPayment * 12) });

  const fixation = metrics.nextFixation;
  const expired = metrics.expiredFixation;

  return (
    <Stats>
      <Stat
        label={t('summary.outstanding')}
        value={formatCurrency(metrics.totalOutstanding)}
        note={`${t('loans', { count: metrics.activeCount })} · ${t('summary.repaidOf', {
          percent: fmt.percent(repaidShare, 0),
          principal: formatCurrency(metrics.totalPrincipal),
        })}`}
      />
      <Stat
        label={t('summary.payments')}
        value={formatCurrency(metrics.totalMonthlyPayment)}
        note={paymentsNote}
      />
      <Stat
        label={t('summary.rate')}
        value={fmt.percent(metrics.averageInterestRate / 100, 2)}
        note={t('summary.rateNote', { interest: formatCurrency(metrics.interestNext12) })}
      />
      <Stat
        label={t('summary.fixation')}
        value={fixation ? text.monthYear(fixation.day) : '—'}
        tone={!fixation && expired ? 'loss' : 'neutral'}
        noteTone="neutral"
        note={
          fixation
            ? t('summary.fixationNote', {
                duration: text.months(monthsAhead(today, fixation.day)),
                name: fixation.row.loan.name,
                rate: fmt.percent((Number(fixation.row.loan.interestRate) || 0) / 100, 2),
              })
            : expired
              ? t('summary.fixationExpired', {
                  name: expired.row.loan.name,
                  month: text.monthYear(expired.day),
                })
              : t('summary.noFixation')
        }
      />
    </Stats>
  );
}
