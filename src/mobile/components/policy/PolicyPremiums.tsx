import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { LinePoint } from '@/components/charts/MoonyLineChart';
import { useFormat } from '@/lib/use-format';
import type { InsuranceRow } from '@/utils/insurance';
import { ScrubChart } from '@/mobile/components/ScrubChart';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { premiumPoints } from '@/mobile/lib/policy-detail';

/** Height of the line (spec §10.4: hero 160, trend 120). */
const CHART_HEIGHT = 120;

interface PolicyPremiumsProps {
  row: InsuranceRow;
  today: number;
}

/**
 * "Zaplaceno na pojistném" (spec §8.8 "Pojistka", prototype frame 14), the last section of the
 * detail: a card on the trend card's pattern, without its segment — "Celkem · 11 plateb", what has
 * been paid in the policy's own currency, and the 120 pt line of the premiums added up, a step at
 * each payment, solid up to today and dashed through the next twelve (the plan). A finger on the
 * line rewrites the label and the figure to that day and says whether it was paid or is planned.
 * A policy that has not started (or has a single reading) has no line, and then no section.
 */
export function PolicyPremiums({ row, today }: PolicyPremiumsProps) {
  const { t } = useTranslation('mobile');
  const { t: ti } = useTranslation('insurance');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const [scrub, setScrub] = useState<LinePoint | null>(null);
  const { policy } = row;
  const { points, paidToDay, paymentsMade } = useMemo(
    () => premiumPoints(policy, today),
    [policy, today]
  );
  if (points.length < 2) return null;

  // The lump sum of a one-time policy is in its own currency, a regular premium in its
  const { currency } = row.premiumNative;
  const money = (value: number) => moneyText(value, currency, { decimals: 0 });
  const label = scrub
    ? `${fmt.day(scrub.t)} · ${scrub.t > today ? ti('detail.chart.tipPlanned') : ti('detail.chart.tipPaid')}`
    : row.frequency === 'one_time'
      ? ti('detail.hero.paidNoteOneTime', { date: fmt.day(policy.startDate) })
      : t('policy.total', { payments: ti('payments', { count: paymentsMade }) });

  return (
    <section>
      <SectionHead title={ti('detail.hero.paid')} note={ti('detail.period.all')} />
      <div className="mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 pb-2 pt-3 shadow-e1">
        <div className="truncate text-m-sub text-ink-3">{label}</div>
        <div className="mb-2 mt-0.5 truncate text-m-title text-ink num">
          {money(scrub ? scrub.value : paidToDay)}
        </div>
        <ScrubChart
          points={points}
          height={CHART_HEIGHT}
          step
          zeroBaseline
          projectedFrom={today}
          onScrub={setScrub}
          label={ti('detail.hero.paid')}
        />
      </div>
    </section>
  );
}
