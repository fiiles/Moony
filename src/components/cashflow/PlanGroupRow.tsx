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
  /** Module labels of the automatic rows in this category, e.g. ["Úvěry"]. */
  modules: string[];
  onToggleExpanded: () => void;
  onToggleInclusion: () => void;
  onAdd: () => void;
}

/**
 * One category of a planning section: whole row toggles its items, the total
 * counts the included rows (an excluded category shows what would count, muted).
 */
export function PlanGroupRow({
  category,
  isIncome,
  expanded,
  reportCurrency,
  modules,
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
    ? category.items.reduce((sum, i) => sum + Number(i.amount), 0)
    : Number(category.total);
  const meta = [
    t('planning.table.groupItems', { count: category.items.length }),
    off
      ? t('planning.table.groupCategoryExcluded')
      : excludedCount > 0
        ? t('planning.table.groupExcluded', { count: excludedCount })
        : null,
    modules.length > 0
      ? t('planning.table.autoFrom', { modules: modules.join(', ') })
      : t('planning.table.manual'),
  ]
    .filter(Boolean)
    .join(' · ');
  return (
    <div
      role="button"
      tabIndex={0}
      aria-expanded={expanded}
      onClick={onToggleExpanded}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onToggleExpanded();
        }
      }}
      className={cn(
        PLAN_ROW_GRID,
        PLAN_ROW_SURFACE,
        'min-h-[52px] cursor-pointer focus-visible:outline-none focus-visible:shadow-focus'
      )}
      data-plan-group={category.key}
    >
      <ChevronRight
        className={cn(
          'size-[14px] text-ink-4 transition-transform duration-fast',
          expanded && 'rotate-90'
        )}
        aria-hidden
      />
      <div className="py-2">
        <b className={cn('block text-table font-650', off ? 'text-ink-4' : 'text-ink')}>{name}</b>
        <small className={cn('block text-micro font-500', off ? 'text-ink-5' : 'text-ink-4')}>
          {meta}
        </small>
      </div>
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
