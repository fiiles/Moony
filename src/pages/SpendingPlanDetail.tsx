import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { useSpendingPlanDetail } from '@/hooks/use-spending-plans';
import { useShellPage } from '@/components/shell/shell-context';
import { BackLink, PageHead } from '@/components/shell/PageHead';
import { PlanChip } from '@/components/spending-plans/PlanChip';

/**
 * A spending plan's detail. Placeholder until the detail page lands (spec §9 "Detail page"):
 * the back link to its year and the plan's name.
 */
export default function SpendingPlanDetail() {
  const { t } = useTranslation('spending_plans');
  const [, params] = useRoute('/spending-plans/:id');
  const { data, isError } = useSpendingPlanDetail(params?.id);
  const plan = data?.summary.plan;

  useShellPage({ crumb: plan?.name });

  return (
    <>
      <BackLink href={plan ? `/spending-plans?year=${plan.year}` : '/spending-plans'}>
        {t('detail.back', { year: plan?.year ?? new Date().getFullYear() })}
      </BackLink>
      {plan ? (
        <PageHead
          eyebrow={[t('detail.eyebrow'), String(plan.year), plan.currency]}
          title={
            <span className="flex items-center gap-3">
              <PlanChip plan={plan} size="lg" />
              <span className="min-w-0 truncate">{plan.name}</span>
            </span>
          }
          description={plan.note ?? undefined}
        />
      ) : (
        isError && <p className="text-table text-ink-3">{t('detail.notFound')}</p>
      )}
    </>
  );
}
