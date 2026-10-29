import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { SpendingPlansOverview } from '@shared/schema';
import { planFlowSegments } from '@/utils/spending-plans';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useFormat } from '@/lib/use-format';
import { Card } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { FlowBar } from '@/components/cashflow/FlowBar';
import { FlowLegend } from '@/components/cashflow/FlowLegend';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';

/**
 * "Kam jdou peníze z plánů": the year's spend by category across all plans on a 100 % bar
 * (100 % = the larger of planned and spent, report currency), the hatched part what is left of
 * the planned amount. Modelled on the cashflow planning card "Kam směřují plánované příjmy".
 */
export function PlansFlowCard({ overview }: { overview: SpendingPlansOverview }) {
  const { t } = useTranslation('spending_plans');
  const fmt = useFormat();
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const { currency, totals, fxQuality, year } = overview;

  const planned = totals.planned === null ? null : parseFloat(totals.planned);
  const spent = totals.spent === null ? null : parseFloat(totals.spent);
  const money = (value: number) => fmt.money(value, currency);
  const percent = (share: number) => fmt.percent(share, 0);

  const segments = useMemo(() => {
    if (planned === null || spent === null) return [];
    const byId = new Map(categories.map((c) => [c.id, c]));
    return planFlowSegments(
      { planned, spent, categories: overview.categories },
      {
        category: (id) => {
          if (id === null) return t('detail.categories.uncategorized');
          const found = byId.get(id);
          return found ? categoryName(found) : '—';
        },
        other: t('flow.other'),
        rest: t('flow.rest'),
      }
    );
  }, [planned, spent, overview.categories, categories, categoryName, t]);

  const known = planned !== null && spent !== null;
  const anySpent = segments.some((s) => s.kind !== 'rest');

  return (
    <Card className="relative mb-7 overflow-hidden px-[21px] pb-[18px] pt-5 after:pointer-events-none after:absolute after:-right-[100px] after:-top-[120px] after:h-[180px] after:w-[340px] after:rounded-full after:bg-hero-orb after:content-['']">
      <div className="relative z-[1] flex items-center justify-between gap-6">
        <div>
          <h3 className="m-0 text-h3 text-ink">{t('flow.title')}</h3>
          <p className="mt-[5px] text-micro font-500 text-ink-4">{t('flow.sub', { year })}</p>
        </div>
        {known &&
          planned > 0 &&
          (spent <= planned ? (
            <Badge>{t('flow.badgeLeft', { percent: percent((planned - spent) / planned) })}</Badge>
          ) : (
            <Badge variant="loss">{t('flow.badgeOver', { amount: money(spent - planned) })}</Badge>
          ))}
      </div>
      {!known ? (
        <ConversionQualityNote
          quality={fxQuality}
          currency={currency}
          className="relative z-[1] mt-4"
        />
      ) : anySpent ? (
        <div className="relative z-[1]">
          <FlowBar
            className="mb-[14px] mt-[18px]"
            segments={segments}
            formatValue={money}
            formatShare={percent}
          />
          <FlowLegend segments={segments} formatValue={money} formatShare={percent} />
        </div>
      ) : (
        <p className="relative z-[1] mt-4 text-table text-ink-3">{t('flow.none')}</p>
      )}
    </Card>
  );
}
