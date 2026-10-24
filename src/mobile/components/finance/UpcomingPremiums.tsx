import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { daysBetween, relativeLabel } from '@/utils/milestones';
import type { InsuranceRow } from '@/utils/insurance';
import { DateChip, DATE_CHIP_WIDTH } from '@/mobile/components/DateChip';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { MoreRow } from '@/mobile/components/wealth/MoreRow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { UPCOMING_DAYS, upcomingPremiums } from '@/mobile/lib/upcoming-premiums';
import { policyPath } from '@/mobile/shell/routes';

/** Payments shown before the rest folds behind "Zobrazit všech N". */
const SHOWN = 5;

interface UpcomingPremiumsProps {
  /** The policies the list reads: the Finance tab's rows after the type filter. */
  rows: readonly InsuranceRow[];
  /** The UTC day the distances count from (`useInsurance().today`). */
  today: number;
}

/**
 * "Nejbližší platby" under the policy rows (spec §8.4, §8.8): the premiums falling due in the next
 * 90 days as a dated list in place of the desktop's 12-month calendar chart — a date chip (the day
 * and its distance), the policy's name and insurer, the amount in the policy's own currency
 * (never converted, never summed; masked with the eye). Five rows, then "Zobrazit všech N" unfolds
 * the rest in place; a tap opens the policy. Nothing is rendered without a payment ahead (every
 * policy ended, or only one-time premiums).
 */
export function UpcomingPremiums({ rows, today }: UpcomingPremiumsProps) {
  const { t } = useTranslation('mobile');
  const moneyText = useMoneyText();
  const [expanded, setExpanded] = useState(false);
  const payments = useMemo(() => upcomingPremiums(rows, today), [rows, today]);
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
          return (
            <EntityRow
              key={`${payment.policyId}:${payment.day}`}
              chip={
                <DateChip
                  day={payment.day}
                  caption={t(`milestones:relative.${distance.key}`, { count: distance.count })}
                />
              }
              chipWidth={DATE_CHIP_WIDTH}
              title={payment.name}
              sub={payment.provider}
              value={moneyText(payment.amount, payment.currency, { decimals: 0 })}
              href={policyPath(payment.policyId)}
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
