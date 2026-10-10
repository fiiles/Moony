import { useTranslation } from 'react-i18next';
import { ChevronRight } from 'lucide-react';
import type { CashflowCategory } from '@shared/schema';
import { Badge } from '@/components/ui/badge';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { PlanRowMenu } from '@/components/cashflow/PlanRowMenu';
import { PLAN_ROW_GRID, PLAN_ROW_SURFACE } from '@/components/cashflow/PlanItemRow';

interface PlanGroupRowProps {
  category: CashflowCategory;
  isIncome: boolean;
  expanded: boolean;
  reportCurrency: string;
  /** Module label of the automatic rows in this category, e.g. "Úvěry". */
  moduleLabel?: string;
  /** id of the element that holds the items (`aria-controls`). */
  controlsId: string;
  onToggleExpanded: () => void;
  onToggleInclusion: () => void;
  onAdd: () => void;
}

/**
 * One category of a planning section. The chevron and name form a real button
 * (`aria-expanded`); a click anywhere else on the row toggles too. The total
 * counts the included rows; an excluded category shows, muted, what would count
 * once it is back in the plan.
 */
export function PlanGroupRow({
  category,
  isIncome,
  expanded,
  reportCurrency,
  moduleLabel,
  controlsId,
  onToggleExpanded,
  onToggleInclusion,
  onAdd,
}: PlanGroupRowProps) {
  const { t } = useTranslation('reports');
  const { signedMoney } = useReportMoney(reportCurrency);
  const off = !category.included;
  const name = t(`categories.${category.key}`, { defaultValue: category.name });
  const excludedCount = category.items.filter((i) => !i.included).length;
  const shownTotal = off
    ? category.items.filter((i) => i.selfIncluded).reduce((sum, i) => sum + Number(i.amount), 0)
    : Number(category.total);
  const meta = [
    t('planning.table.groupItems', { count: category.items.length }),
    off
      ? t('planning.table.groupCategoryExcluded')
      : excludedCount > 0
        ? t('planning.table.groupExcluded', { count: excludedCount })
        : null,
    moduleLabel
      ? t('planning.table.autoFrom', { modules: moduleLabel })
      : t('planning.table.manual'),
  ]
    .filter(Boolean)
    .join(' · ');
  return (
    <div
      className={cn(PLAN_ROW_GRID, PLAN_ROW_SURFACE, 'min-h-[52px] cursor-pointer')}
      onClick={onToggleExpanded}
      data-plan-group={category.key}
    >
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={controlsId}
        className="col-span-2 grid grid-cols-[16px_1fr] items-center gap-x-2.5 rounded-r1 py-2 text-left focus-visible:outline-none focus-visible:shadow-focus"
      >
        <ChevronRight
          className={cn(
            'size-[14px] text-ink-4 transition-transform duration-fast',
            expanded && 'rotate-90'
          )}
          aria-hidden
        />
        <span>
          <b className={cn('block text-table font-650', off ? 'text-ink-4' : 'text-ink')}>{name}</b>
          <small className="block text-micro font-500 text-ink-4">{meta}</small>
        </span>
      </button>
      <span
        className={cn(
          'inline-flex items-center gap-2.5 whitespace-nowrap text-table font-650 num',
          off ? 'text-ink-4' : isIncome ? 'text-gain' : 'text-ink'
        )}
      >
        {off && <Badge variant="outline">{t('planning.table.outOfPlan')}</Badge>}
        {signedMoney(isIncome ? shownTotal : -shownTotal)}
      </span>
      <div data-row-actions className="flex justify-end">
        <PlanRowMenu
          name={name}
          kind="category"
          included={category.included}
          onToggleInclusion={onToggleInclusion}
          onAdd={onAdd}
        />
      </div>
    </div>
  );
}
