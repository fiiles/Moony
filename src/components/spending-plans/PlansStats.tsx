import { useTranslation } from 'react-i18next';
import type { SpendingPlansOverview } from '@shared/schema';
import type { PlanRow } from '@/utils/spending-plans';
import { useFormat } from '@/lib/use-format';
import { Stat, Stats } from '@/components/common/Stat';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { PlanMoney } from '@/components/spending-plans/PlanMoney';

/**
 * The overview's four stats (prototype "Výdajové plány"): planned for the year, spent, left in
 * the active plans and what needs attention. Totals are in the report currency; a plan that
 * cannot be converted leaves them (marked, and named in the note under the stats).
 */
export function PlansStats({
  overview,
  rows,
}: {
  overview: SpendingPlansOverview;
  rows: PlanRow[];
}) {
  const { t } = useTranslation('spending_plans');
  const fmt = useFormat();
  const { currency, totals, fxQuality, year } = overview;

  const closedCount = rows.filter((r) => r.closed).length;
  const active = rows.filter((r) => !r.closed);
  const overCount = active.filter((r) => r.status === 'over').length;
  const nearCount = active.filter((r) => r.status === 'near').length;

  const planned = totals.planned === null ? null : parseFloat(totals.planned);
  const spent = totals.spent === null ? null : parseFloat(totals.spent);
  const remaining = totals.remainingActive === null ? null : parseFloat(totals.remainingActive);
  const money = (amount: string | null, signedNegative = false) => (
    <PlanMoney
      amount={amount}
      currency={currency}
      quality={fxQuality}
      signedNegative={signedNegative}
    />
  );

  return (
    <>
      <Stats>
        <Stat
          label={t('stats.planned', { year })}
          value={money(totals.planned)}
          note={[
            t('stats.plans', { count: rows.length }),
            closedCount > 0 ? t('stats.closed', { count: closedCount }) : null,
          ]
            .filter(Boolean)
            .join(' · ')}
        />
        <Stat
          label={t('stats.spent')}
          value={money(totals.spent)}
          note={
            planned !== null && planned > 0 && spent !== null
              ? t('stats.spentShare', { percent: fmt.percent(spent / planned, 0) })
              : undefined
          }
        />
        <Stat
          label={t('stats.remaining')}
          value={money(totals.remainingActive, true)}
          tone={remaining !== null && remaining < 0 ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={t('stats.activePlans', { count: active.length })}
        />
        <Stat
          label={t('stats.attention')}
          value={overCount > 0 ? t('stats.over', { count: overCount }) : t('stats.allWithin')}
          tone={overCount > 0 ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={nearCount > 0 ? t('stats.near', { count: nearCount }) : t('stats.noneNear')}
        />
      </Stats>
      <ConversionQualityNote quality={fxQuality} currency={currency} />
    </>
  );
}
