import { Fragment, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { ChevronRight, Ellipsis, Pencil, Plus, RotateCcw, Trash2 } from 'lucide-react';
import type { BudgetGoal, BudgetGoalInterval, TransactionCategory } from '@shared/schema';
import { budgetingApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { useCategoryName } from '@/hooks/use-categories';
import { useDeleteBudgetIntervals, useEndBudgetGoal } from '@/hooks/use-budget-goal-mutations';
import { formatReportMoney } from '@/utils/report-currencies';
import {
  budgetOverview,
  currentMonthStart,
  type BudgetLimitPeriod,
  type BudgetOverviewRow,
} from '@/utils/budget-goal-versions';
import { cn } from '@/lib/utils';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { ReportError } from '@/components/reports/ReportQualityNote';
import { StatSkeleton } from '@/components/common/Stat';

interface BudgetManageDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Goals with a stored goal row (the ids `delete_budget_goal` accepts). */
  activeGoals: readonly BudgetGoal[];
  categories: readonly TransactionCategory[];
  /** Some expense category has no limit yet. */
  canAdd: boolean;
  /** Add a limit, optionally for a preselected category. */
  onAdd: (categoryId?: string) => void;
  /** Edit a goal from `effectiveMonth`, or from the month the editor picks itself. */
  onEdit: (goal: BudgetGoal, effectiveMonth?: number) => void;
}

/**
 * "Spravovat rozpočty" (design system §6 Modal, §7 Planning): one row per
 * category with the limit in effect (or the first scheduled one), its
 * upcoming changes, earlier limits behind a chevron and ended budgets folded
 * at the bottom. Gaps without a limit are never rows of their own.
 */
export function BudgetManageDialog({
  open,
  onOpenChange,
  activeGoals,
  categories,
  canAdd,
  onAdd,
  onEdit,
}: BudgetManageDialogProps) {
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const categoryName = useCategoryName();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [showEnded, setShowEnded] = useState(false);
  const [pendingEnd, setPendingEnd] = useState<{ id: string; name: string } | null>(null);
  const [pendingInterval, setPendingInterval] = useState<{
    interval: BudgetGoalInterval;
    name: string;
    amount: string;
    period: string;
  } | null>(null);

  const {
    data: history = [],
    isLoading,
    isError,
    refetch,
  } = useQuery({
    queryKey: ['budget-goal-history'],
    queryFn: () => budgetingApi.getBudgetGoalHistory(),
    enabled: open,
  });

  const endMutation = useEndBudgetGoal(() => setPendingEnd(null));
  const deleteMutation = useDeleteBudgetIntervals(() => setPendingInterval(null));

  const rows = useMemo(() => {
    const list = budgetOverview(history, currentMonthStart()).map((row) => {
      const category = categories.find((c) => c.id === row.goal.categoryId);
      return { ...row, name: category ? categoryName(category) : row.goal.categoryId };
    });
    return list.sort((a, b) => a.name.localeCompare(b.name, fmt.locale));
  }, [history, categories, categoryName, fmt.locale]);
  const live = rows.filter((row) => row.status !== 'ended');
  const ended = rows.filter((row) => row.status === 'ended');

  const monthLabel = (value: number) => fmt.monthInSentence(new Date(value * 1000));
  const money = (amount: string | null, currency: string) =>
    amount == null ? t('noBudget') : formatReportMoney(fmt, Number(amount), currency);
  // An open row says only where it starts: what follows is in its sub-line.
  const validity = ({ version, end }: BudgetLimitPeriod, open = false) => {
    const start = monthLabel(version.effectiveMonth);
    if (end == null || open) return t('settings.from', { month: start });
    const last = monthLabel(end);
    return start === last ? start : t('settings.between', { start, end: last });
  };
  const rowKey = (goal: BudgetGoal) => `${goal.categoryId}:${goal.timeframe}`;
  const goalId = (goal: BudgetGoal) =>
    activeGoals.find((g) => g.categoryId === goal.categoryId && g.timeframe === goal.timeframe)
      ?.id ?? null;
  const toggle = (key: string) =>
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const rowMenu = (items: React.ReactNode) => (
    <div data-row-actions>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label={tc('labels.moreActions')}>
            <Ellipsis />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">{items}</DropdownMenuContent>
      </DropdownMenu>
    </div>
  );

  const renderRow = (row: BudgetOverviewRow & { name: string }) => {
    const key = rowKey(row.goal);
    const isOpen = expanded.has(key);
    const id = goalId(row.goal);
    return (
      <Fragment key={key}>
        <TableRow className={cn('h-[52px]', row.status === 'ended' && 'text-ink-3')}>
          <TableCell>
            <div className="flex items-start gap-1.5">
              {row.history.length > 0 ? (
                <button
                  type="button"
                  aria-expanded={isOpen}
                  aria-label={t('manage.showHistory', { category: row.name })}
                  onClick={() => toggle(key)}
                  className="mt-px grid size-5 shrink-0 place-items-center rounded-r1 text-ink-4 hover:text-ink focus-visible:outline-none focus-visible:shadow-focus"
                >
                  <ChevronRight
                    className={cn('size-[14px] transition-transform', isOpen && 'rotate-90')}
                  />
                </button>
              ) : (
                <span className="size-5 shrink-0" />
              )}
              <div className="min-w-0">
                <span
                  className={cn(
                    'block truncate font-650',
                    row.status === 'ended' ? 'text-ink-3' : 'text-ink'
                  )}
                >
                  {row.name}
                </span>
                {row.upcoming.map((version) => (
                  <small
                    key={version.effectiveMonth}
                    className="mt-0.5 block text-micro font-500 text-ink-4"
                  >
                    {version.amount == null
                      ? t('readiness.scheduledRemoval', {
                          month: monthLabel(version.effectiveMonth),
                        })
                      : t('readiness.scheduledLimit', {
                          month: monthLabel(version.effectiveMonth),
                          amount: money(version.amount, version.currency),
                        })}
                  </small>
                ))}
              </div>
            </div>
          </TableCell>
          <TableCell
            className={cn(
              'text-right font-650 num',
              row.status === 'ended' ? 'text-ink-3' : 'text-ink'
            )}
          >
            {money(row.main.version.amount, row.main.version.currency)}
            {row.goal.timeframe !== 'monthly' && (
              <small className="mt-0.5 block text-micro font-500 text-ink-4">
                {t(row.goal.timeframe)}
              </small>
            )}
          </TableCell>
          <TableCell>
            <span className="flex flex-wrap items-center gap-2">
              {validity(row.main, row.status !== 'ended')}
              {row.status === 'scheduled' && <Badge>{t('manage.scheduled')}</Badge>}
            </span>
          </TableCell>
          <TableCell className="pr-3 text-right">
            {rowMenu(
              row.status === 'ended' ? (
                <DropdownMenuItem onSelect={() => onAdd(row.goal.categoryId)}>
                  <RotateCcw />
                  {t('manage.restore')}
                </DropdownMenuItem>
              ) : (
                <>
                  <DropdownMenuItem onSelect={() => onEdit(row.goal)}>
                    <Pencil />
                    {t('actions.editLimit')}
                  </DropdownMenuItem>
                  {id && (
                    <DropdownMenuItem
                      variant="danger"
                      onSelect={() => setPendingEnd({ id, name: row.name })}
                    >
                      <Trash2 />
                      {t('manage.end')}
                    </DropdownMenuItem>
                  )}
                </>
              )
            )}
          </TableCell>
        </TableRow>
        {isOpen &&
          row.history.map((period) => (
            <TableRow
              key={`${key}:${period.version.effectiveMonth}`}
              className="h-[40px] bg-well text-ink-3"
            >
              <TableCell className="pl-[42px] text-micro">{t('manage.earlierLimit')}</TableCell>
              <TableCell className="text-right num">
                {money(period.version.amount, period.version.currency)}
              </TableCell>
              <TableCell>{validity(period)}</TableCell>
              <TableCell className="pr-3 text-right">
                {rowMenu(
                  <>
                    <DropdownMenuItem
                      onSelect={() => onEdit(row.goal, period.version.effectiveMonth)}
                    >
                      <Pencil />
                      {t('actions.editLimit')}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      variant="danger"
                      onSelect={() =>
                        setPendingInterval({
                          interval: {
                            categoryId: row.goal.categoryId,
                            timeframe: row.goal.timeframe,
                            effectiveMonth: period.version.effectiveMonth,
                          },
                          name: row.name,
                          amount: money(period.version.amount, period.version.currency),
                          period: validity(period),
                        })
                      }
                    >
                      <Trash2 />
                      {t('manage.deleteInterval')}
                    </DropdownMenuItem>
                  </>
                )}
              </TableCell>
            </TableRow>
          ))}
      </Fragment>
    );
  };

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-[720px]">
          <DialogHeader>
            <DialogTitle>{t('manage.title')}</DialogTitle>
            <DialogDescription>{t('manage.description')}</DialogDescription>
          </DialogHeader>
          {isError ? (
            <ReportError onRetry={() => void refetch()} />
          ) : isLoading ? (
            <StatSkeleton />
          ) : rows.length === 0 ? (
            <div className="py-6 text-center">
              <p className="font-650 text-ink">{t('noBudgets.title')}</p>
              <p className="mt-1 text-table text-ink-3">{t('noBudgets.description')}</p>
            </div>
          ) : (
            <div className="-mx-[27px]">
              <Table containerClassName="max-h-[460px] overflow-y-auto" className="table-fixed">
                <TableHeader className="sticky top-0 z-[1] bg-well [&_th]:shadow-[inset_0_-1px_0_var(--line)] [&_tr]:border-b-0">
                  <TableRow>
                    <TableHead className="w-[40%]">{t('table.category')}</TableHead>
                    <TableHead className="w-[20%] text-right">{t('manage.limit')}</TableHead>
                    <TableHead>{t('manage.validity')}</TableHead>
                    <TableHead className="w-12">
                      <span className="sr-only">{tc('labels.actions')}</span>
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {live.map(renderRow)}
                  {ended.length > 0 && (
                    <TableRow className="h-[44px] hover:bg-transparent">
                      <TableCell colSpan={4}>
                        <button
                          type="button"
                          aria-expanded={showEnded}
                          onClick={() => setShowEnded((v) => !v)}
                          className="flex items-center gap-1.5 text-caption font-650 text-ink-3 hover:text-ink focus-visible:outline-none focus-visible:underline"
                        >
                          <ChevronRight
                            className={cn(
                              'size-[14px] transition-transform',
                              showEnded && 'rotate-90'
                            )}
                          />
                          {t('manage.endedBudgets', { count: ended.length })}
                        </button>
                      </TableCell>
                    </TableRow>
                  )}
                  {showEnded && ended.map(renderRow)}
                </TableBody>
              </Table>
            </div>
          )}
          <DialogFooter>
            <Button variant="ghost" onClick={() => onOpenChange(false)}>
              {tc('buttons.close')}
            </Button>
            <Button onClick={() => onAdd()} disabled={!canAdd}>
              <Plus />
              {t('actions.add')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDeleteDialog
        open={!!pendingEnd}
        onOpenChange={(o) => !o && setPendingEnd(null)}
        title={t('manage.endTitle')}
        description={t('manage.endDescription', { category: pendingEnd?.name ?? '' })}
        confirmLabel={t('manage.end')}
        isPending={endMutation.isPending}
        onConfirm={() => pendingEnd && endMutation.mutate(pendingEnd.id)}
      />
      <ConfirmDeleteDialog
        open={!!pendingInterval}
        onOpenChange={(o) => !o && setPendingInterval(null)}
        title={t('manage.deleteIntervalTitle')}
        description={
          pendingInterval
            ? t('manage.deleteIntervalDescription', {
                category: pendingInterval.name,
                amount: pendingInterval.amount,
                period: pendingInterval.period,
              })
            : ''
        }
        confirmLabel={tc('buttons.delete')}
        isPending={deleteMutation.isPending}
        onConfirm={() => pendingInterval && deleteMutation.mutate([pendingInterval.interval])}
      />
    </>
  );
}
