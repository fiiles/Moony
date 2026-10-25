import { useTranslation } from 'react-i18next';
import { useLoanText } from '@/hooks/use-loan-text';
import { useFormat } from '@/lib/use-format';
import { annuityPayment } from '@/utils/loan-trajectory';
import { monthsAhead, type LoanRow } from '@/utils/loans';
import { StatRow } from '@/mobile/components/StatRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import type { LoanDetailStats } from '@/mobile/lib/loan-detail';

/** Whole amounts: the stats show no decimals. */
const WHOLE = { decimals: 0 } as const;

interface LoanStatsProps {
  row: LoanRow;
  stats: LoanDetailStats;
  today: number;
}

/**
 * The four stats of the loan, two rows of two (spec §8.8 "Úvěr", prototype frame 13): Zaplacené
 * úroky, Splaceno z jistiny, Zbývá splátek and Konec fixace — the desktop's numbers and notes
 * (`LoanDetail.tsx`), the notes short enough for a half-width card. Amounts go through the eye,
 * counts, percents and months stay.
 */
export function LoanStats({ row, stats, today }: LoanStatsProps) {
  const { t } = useTranslation('mobile');
  const { t: tl } = useTranslation('loans');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const text = useLoanText();
  const { loan, currency, trajectory } = row;
  const money = (value: number) => moneyText(value, currency, WHOLE);
  const rate = Number(loan.interestRate) || 0;
  const lastDue = trajectory.upcoming[trajectory.upcoming.length - 1]?.dueDay ?? null;

  const interest = {
    label: tl('detail.stats.interest'),
    value: money(stats.interestPaid),
    note: stats.started
      ? t('loan.stats.since', { month: text.monthYear(stats.startDay) })
      : t('loan.notStarted', { date: fmt.day(loan.startDate) }),
  };

  const principal = {
    label: tl('detail.stats.principal'),
    value: money(stats.principalRepaid),
    note: [
      fmt.percent(stats.repaidShare, 0),
      stats.extraPaid > 0 ? t('loan.stats.extra', { amount: money(stats.extraPaid) }) : null,
    ]
      .filter(Boolean)
      .join(' · '),
  };

  const remainingNote = (() => {
    // A loan that has not started owes nothing yet, but it is not paid off
    if (stats.started && stats.balance <= 0) return tl('detail.stats.paidOff');
    if (stats.endBalance !== null) {
      return tl('detail.stats.remainingToEnd', {
        month: text.monthYear(loan.endDate!),
        amount: money(stats.endBalance),
        interest: money(stats.remainingInterest),
      });
    }
    const end = stats.payoffDay ?? lastDue;
    return stats.remaining !== null && end !== null
      ? t('loan.stats.until', { month: text.monthYear(end) })
      : tl('detail.stats.remainingNone');
  })();
  const remaining = {
    label: tl('detail.stats.remaining'),
    value: stats.remaining !== null ? fmt.number(stats.remaining) : '—',
    note: remainingNote,
  };

  // The fixation still ahead; one that has ended (the validity day is today or past) says so; a
  // loan without a validity date has a fixed rate, as the Finance list words it
  const fixation = {
    label: tl('detail.stats.fixation'),
    ...(stats.fixationEnd !== null
      ? {
          value: text.monthYear(stats.fixationEnd),
          note: tl('detail.stats.fixationNote', {
            duration: text.months(monthsAhead(today, stats.fixationEnd)),
            up: fmt.percent((rate + 1) / 100, 0),
            upPayment: money(annuityPayment(stats.balance, rate + 1, stats.remaining ?? 360)),
            down: fmt.percent(Math.max(0, rate - 1) / 100, 0),
            downPayment: money(
              annuityPayment(stats.balance, Math.max(0, rate - 1), stats.remaining ?? 360)
            ),
          }),
        }
      : stats.fixationEnded !== null
        ? {
            value: text.monthYear(stats.fixationEnded),
            note: tl('table.fixationEnded', { month: text.monthYear(stats.fixationEnded) }),
          }
        : { value: '—', note: tl('table.fixedRate') }),
  };

  return (
    // The two rows sit 10 pt apart, as the stats within a row do
    <div className="[&>:first-child]:mb-2.5">
      <StatRow stats={[interest, principal]} />
      <StatRow stats={[remaining, fixation]} />
    </div>
  );
}
