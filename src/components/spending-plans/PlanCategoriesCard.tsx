import { useTranslation } from 'react-i18next';
import type { TransactionCategory } from '@shared/schema';
import type { PlanCategoryRow } from '@/utils/spending-plans';
import { useCategoryName } from '@/hooks/use-categories';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Progress } from '@/components/ui/progress';
import { CategoryIconChip } from '@/components/common/CategoryIcon';
import { FilterBadge } from '@/components/common/FilterBadge';

interface PlanCategoriesCardProps {
  rows: PlanCategoryRow[];
  currency: string;
  categoryById: Map<string, TransactionCategory>;
  /** Key of the chosen category (`categoryKey`), null without a category filter. */
  selected: string | null;
  onToggle: (key: string) => void;
  /** The month filter as written in the badge ("červenec"); null without one. */
  monthLabel: string | null;
  onClearMonth: () => void;
  /** The plan has payments at all (decides the empty sentence). */
  hasPayments: boolean;
}

/**
 * "Kategorie" (spec §9, O14): the plan's spend per payment category with each category's colored
 * chip, payment count, amount, share and a bar; a row toggles the category filter of the chart
 * and the payments. The month filter narrows it (dark filter badge in the head).
 */
export function PlanCategoriesCard({
  rows,
  currency,
  categoryById,
  selected,
  onToggle,
  monthLabel,
  onClearMonth,
  hasPayments,
}: PlanCategoriesCardProps) {
  const { t } = useTranslation('spending_plans');
  const fmt = useFormat();
  const categoryName = useCategoryName();
  const total = rows.reduce((sum, row) => sum + row.spent, 0);

  return (
    <Card variant="table" className={cn(monthLabel && 'border-line-hover')}>
      <CardHeader>
        <div className="min-w-0">
          <CardTitle className="flex flex-wrap items-center gap-2">
            {t('detail.categories.title')}
            <FilterBadge
              labels={monthLabel ? [monthLabel] : []}
              onClear={onClearMonth}
              title={t('detail.filterTitle')}
            />
          </CardTitle>
          <CardDescription>{t('detail.categories.sub')}</CardDescription>
        </div>
      </CardHeader>
      {rows.length === 0 ? (
        <p className="m-0 px-[21px] pb-8 pt-3 text-center text-table text-ink-3">
          {hasPayments ? t('detail.categories.emptyMonth') : t('detail.categories.empty')}
        </p>
      ) : (
        <div className="px-[9px] pb-2.5">
          {rows.map((row) => {
            const category = row.categoryId ? categoryById.get(row.categoryId) : undefined;
            const name = category ? categoryName(category) : t('detail.categories.uncategorized');
            const share = total > 0 ? Math.max(0, row.spent / total) : 0;
            const on = row.key === selected;
            return (
              <button
                key={row.key}
                type="button"
                aria-pressed={on}
                onClick={() => onToggle(row.key)}
                className={cn(
                  'grid w-full grid-cols-[28px_minmax(0,1fr)_auto] items-center gap-x-2.5 gap-y-[5px] rounded-r2 px-3 py-[11px] text-left text-table text-ink-2 transition-colors duration-fast hover:bg-well focus-visible:outline-none focus-visible:shadow-focus',
                  on && 'bg-well-2 hover:bg-well-2'
                )}
              >
                <CategoryIconChip category={category ?? null} />
                <span className="min-w-0 truncate">
                  <span className={cn(on && 'font-650 text-ink')}>{name}</span>
                  <small className="ml-1.5 text-micro font-500 text-ink-4">
                    {t('detail.categories.payments', { count: row.count })}
                  </small>
                </span>
                <span className="whitespace-nowrap">
                  <span className="font-650 text-ink num">{fmt.money(row.spent, currency)}</span>
                  <span className="text-ink-4"> · {fmt.percent(share, 0)}</span>
                </span>
                <Progress value={share * 100} className="col-start-2 col-end-4 h-[5px]" />
              </button>
            );
          })}
          <div className="mt-1 flex justify-between border-t border-line-soft px-3 pb-1 pt-3 text-table text-ink-3">
            <span className="pl-[38px]">
              {monthLabel
                ? t('detail.categories.totalIn', { month: monthLabel })
                : t('detail.categories.total')}
            </span>
            <b className="text-ink num">{fmt.money(total, currency)}</b>
          </div>
        </div>
      )}
    </Card>
  );
}
