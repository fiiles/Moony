import { useTranslation } from 'react-i18next';
import type { CashflowReportItem } from '@shared/schema';
import { Badge } from '@/components/ui/badge';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import { PlanRowMenu, type PlanModuleLink } from '@/components/cashflow/PlanRowMenu';

/** Chevron column · name and sub-line · amount · "···". */
export const PLAN_ROW_GRID =
  'grid grid-cols-[16px_1fr_auto_28px] items-center gap-x-2.5 pl-[14px] pr-[10px]';

/** Row surface shared by group and item rows: hover well, "···" on hover / focus-within / open menu. */
export const PLAN_ROW_SURFACE =
  'group/row border-b border-line-soft transition-colors duration-fast hover:bg-well focus-within:bg-well [&_[data-row-actions]]:opacity-0 [&_[data-row-actions]]:transition-opacity [&_[data-row-actions]]:duration-fast [&:hover_[data-row-actions]]:opacity-100 [&:focus-within_[data-row-actions]]:opacity-100 [&:has([data-state=open])_[data-row-actions]]:opacity-100';

interface PlanItemRowProps {
  item: CashflowReportItem;
  isIncome: boolean;
  categoryIncluded: boolean;
  reportCurrency: string;
  viewType: 'monthly' | 'yearly';
  /** Name of the automatic source a manual replacement stands in for. */
  replacedSourceName?: string;
  module?: PlanModuleLink;
  onToggleInclusion: () => void;
  onReplace: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

/** One planning item under its category row (46 px dense row). */
export function PlanItemRow({
  item,
  isIncome,
  categoryIncluded,
  reportCurrency,
  viewType,
  replacedSourceName,
  module,
  onToggleInclusion,
  onReplace,
  onEdit,
  onDelete,
}: PlanItemRowProps) {
  const { t } = useTranslation('reports');
  const { signedMoney, nativeMoney } = useReportMoney(reportCurrency);
  const off = !item.included;
  const incomplete = item.fxQuality === 'missing';
  const unavailable = incomplete && item.convertedComponentCount === 0;
  const original =
    item.originalCurrency === 'MULTI'
      ? item.originalComponents
          .map((component) => {
            const frequency =
              component.frequency === 'yearly'
                ? t('planning.table.perYear')
                : component.frequency === 'monthly'
                  ? t('planning.table.perMonth')
                  : '';
            return `${nativeMoney(Number(component.amount), component.currency)}${frequency ? ` · ${frequency}` : ''}`;
          })
          .join(' · ')
      : item.originalFrequency === 'yearly'
        ? `${nativeMoney(Number(item.originalAmount), item.originalCurrency)} · ${t('planning.table.perYear')}`
        : viewType === 'yearly'
          ? `${nativeMoney(Number(item.originalAmount), item.originalCurrency)} · ${t('planning.table.perMonth')}`
          : t('planning.table.perMonth');
  const origin = item.isUserDefined
    ? t('planning.table.manual')
    : t('planning.table.auto', { module: module?.label ?? '' });
  return (
    <div
      className={cn(PLAN_ROW_GRID, PLAN_ROW_SURFACE, 'min-h-[46px] bg-paper')}
      data-plan-item={item.id}
    >
      <span aria-hidden />
      <div className="py-2">
        <b className={cn('block text-table font-650', off ? 'text-ink-4' : 'text-ink')}>
          {item.name}
        </b>
        {replacedSourceName && (
          <small className="block text-micro font-500 text-ink-4">
            {t('readiness.replacing', { name: replacedSourceName })}
          </small>
        )}
        <small className="block text-micro font-500 text-ink-4">
          {origin} · {original}
        </small>
      </div>
      <span
        className={cn(
          'inline-flex items-center gap-2.5 whitespace-nowrap text-table font-650 num',
          off ? 'text-ink-4' : isIncome ? 'text-gain' : 'text-ink'
        )}
      >
        {off && categoryIncluded && (
          <Badge variant="outline">{t('planning.table.outOfPlan')}</Badge>
        )}
        <span className="text-right">
          {unavailable
            ? t('readiness.fxUnavailable')
            : signedMoney(isIncome ? Number(item.amount) : -Number(item.amount))}
          {incomplete && !unavailable && (
            // A row out of the plan no longer affects any total: its warning is muted.
            <small className={cn('block text-micro', off ? 'text-ink-4' : 'text-loss')}>
              {t('readiness.partialAmount')}
            </small>
          )}
        </span>
      </span>
      <div data-row-actions className="flex justify-end">
        <PlanRowMenu
          name={item.name}
          kind={item.isUserDefined ? 'manual' : 'auto'}
          included={item.included}
          categoryIncluded={categoryIncluded}
          module={module}
          onToggleInclusion={onToggleInclusion}
          onReplace={onReplace}
          onEdit={onEdit}
          onDelete={onDelete}
        />
      </div>
    </div>
  );
}
