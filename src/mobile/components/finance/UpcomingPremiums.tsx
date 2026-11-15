import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import { daysBetween, relativeLabel } from '@/utils/milestones';
import type { InsuranceRow } from '@/utils/insurance';
import type { LoanRow } from '@/utils/loans';
import { DateChip, DATE_CHIP_WIDTH } from '@/mobile/components/DateChip';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { MoreRow } from '@/mobile/components/wealth/MoreRow';
import { useApprox } from '@/mobile/hooks/use-approx';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import {
  UPCOMING_DAYS,
  upcomingLoanPayments,
  upcomingPremiums,
} from '@/mobile/lib/upcoming-premiums';
import { loanPath, policyPath } from '@/mobile/shell/routes';

const NO_LOANS: readonly LoanRow[] = [];

/** Payments shown before the rest folds behind "Zobrazit všech N". */
const SHOWN = 5;

interface UpcomingPremiumsProps {
  /** The policies the list reads: the Finance tab's rows after the type filter. */
  rows: readonly InsuranceRow[];
  /** The loans the list reads: the Finance tab's loan rows (C11: the heading promises every payment). */
  loans?: readonly LoanRow[];
  /** The UTC day the distances count from (`useInsurance().today`). */
  today: number;
}

/**
 * "Nejbližší platby" under the policy rows (spec §8.4, §8.8): the premiums and the loan payments
 * ("Splátka úvěru" in the sub-line, C11) falling due in the next 90 days as a dated list in place of the desktop's 12-month calendar chart — a date chip (the day
 * over its weekday), the policy's name, the insurer with the distance ("Allianz · za 73 dní") and
 * the amount in the policy's or the loan's own currency (never summed), with "≈" its worth in the
 * main currency under it when that is another one (README §13, "Money and currencies"). The
 * distance is in the sub-line, not in the chip: "in 2 Monaten" does not fit 54 pt, a weekday
 * always does; a long insurer is the part of the sub-line that gives way. Five rows, then "Zobrazit
 * všech N" unfolds the rest in place; a tap opens the policy. Nothing is rendered without a payment
 * ahead (every policy ended, or only one-time premiums).
 */
export function UpcomingPremiums({ rows, loans = NO_LOANS, today }: UpcomingPremiumsProps) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const approx = useApprox();
  const [expanded, setExpanded] = useState(false);
  const loanSub = t('finance.loanPayment');
  // Premiums and loan payments in one dated list, nearest first (a stable merge: premiums first
  // within a day, each part already in its own order)
  const payments = useMemo(
    () =>
      [
        ...upcomingPremiums(rows, today).map((p) => ({
          key: `p:${p.policyId}:${p.day}`,
          name: p.name,
          provider: p.provider,
          day: p.day,
          amount: p.amount,
          currency: p.currency,
          href: policyPath(p.policyId),
        })),
        ...upcomingLoanPayments(loans, today).map((p) => ({
          key: `l:${p.loanId}:${p.day}`,
          name: p.name,
          provider: loanSub,
          day: p.day,
          amount: p.amount,
          currency: p.currency,
          href: loanPath(p.loanId),
        })),
      ].sort((a, b) => a.day - b.day),
    [rows, loans, today, loanSub]
  );
  if (payments.length === 0) return null;

  const shown = expanded ? payments : payments.slice(0, SHOWN);
  return (
    <section>
      <SectionHead
        title={t('finance.insurance.upcoming')}
        note={t('finance.insurance.upcomingNote', { count: UPCOMING_DAYS })}
      />
      <EntityRows>
        {shown.map((payment) => {
          const distance = relativeLabel(daysBetween(today, payment.day));
          const when = t(`milestones:relative.${distance.key}`, { count: distance.count });
          return (
            <EntityRow
              key={payment.key}
              chip={
                <DateChip day={payment.day} caption={fmt.day(payment.day, { weekday: 'short' })} />
              }
              chipWidth={DATE_CHIP_WIDTH}
              title={payment.name}
              sub={
                <span className="flex min-w-0">
                  {payment.provider && (
                    <>
                      <span className="min-w-0 truncate">{payment.provider}</span>
                      <span className="flex-none whitespace-pre"> · </span>
                    </>
                  )}
                  <span className="flex-none">{when}</span>
                </span>
              }
              value={moneyText(payment.amount, payment.currency, { decimals: 0 })}
              valueSub={approx(payment.amount, payment.currency)}
              href={payment.href}
            />
          );
        })}
        {shown.length < payments.length && (
          <MoreRow
            label={t('finance.insurance.showAll', { count: payments.length })}
            onClick={() => setExpanded(true)}
          />
        )}
      </EntityRows>
    </section>
  );
}
