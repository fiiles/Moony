import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Settings2, SlidersHorizontal } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import {
  filterCounts,
  filterRows,
  sortRows,
  type ExpenseFilter,
  type ExpenseRow,
  type ExpenseSort,
  type RowLimit,
} from '@/utils/cashflow-budget-rows';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { Segmented } from '@/components/ui/segmented';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { CategoryIconChip } from '@/components/common/CategoryIcon';

/** What the limit cell can say beside the amount, from the stored versions of the budget. */
export interface LimitNote {
  /** The limit covers only some months of the quarter or year. */
  partial: boolean;
}

export interface ExpensesByCategoryCardProps {
  rows: readonly ExpenseRow[];
  /** The period's expenses in the report currency (the top stat). */
  total: number;
  label: (row: ExpenseRow) => string;
  category: (row: ExpenseRow) => TransactionCategory | null;
  /** A limit can be set on this row (an expense category). */
  canSetLimit: (row: ExpenseRow) => boolean;
  limitNote: (row: ExpenseRow) => LimitNote | null;
  /** The stored budgets have loaded, so a limit action knows whether to add or edit. */
  limitsReady: boolean;
  /** Report amounts; `native` formats an amount in its own currency. */
  money: (amount: number) => string;
  native: (amount: number, currency: string, opts?: { signed?: boolean }) => string;
  /** "Limity vyhodnocené za říjen 2026". */
  evaluationNote: string;
  /** How far the period has run, 0–100, while it contains today; marks the usage bars. */
  pace?: number;
  /** The rows still belong to the previous period: nothing opens. */
  busy: boolean;
  onOpenTransactions: (row: ExpenseRow) => void;
  onLimit: (row: ExpenseRow) => void;
  onManageCategories: () => void;
  onManageLimits: () => void;
}

/**
 * Beside the right column (1240–1439 px) the card is too narrow for every column: "Zbývá" moves
 * under the usage bar.
 */
const WIDE_ONLY = 'min-[1240px]:max-[1439px]:hidden';
const NARROW_ONLY = 'hidden min-[1240px]:max-[1439px]:block';

const STATE_KEYS: Record<RowLimit['state'], string> = {
  over: 'table.state.overShort',
  exhausted: 'table.state.exhausted',
  near: 'table.state.near',
  ok: 'table.state.ok',
  unavailable: 'readiness.usageUnavailable',
};

/**
 * "Výdaje podle kategorií" of the Cashflow screen (spec
 * `2026-10-28-cashflow-with-budgets-implementation-design.md` §4): each expense line with what was
 * spent in the report currency and, where the period has one, its limit evaluated in the budget's
 * own currency. Filters and sorting only change what this table shows.
 */
export function ExpensesByCategoryCard({
  rows,
  total,
  label,
  category,
  canSetLimit,
  limitNote,
  limitsReady,
  money,
  native,
  evaluationNote,
  pace,
  busy,
  onOpenTransactions,
  onLimit,
  onManageCategories,
  onManageLimits,
}: ExpensesByCategoryCardProps) {
  const { t } = useTranslation('reports');
  const { t: tb } = useTranslation('budgeting');
  const fmt = useFormat();
  const [filter, setFilter] = useState<ExpenseFilter>('all');
  const [sort, setSort] = useState<ExpenseSort>('amount');

  const counts = useMemo(() => filterCounts(rows), [rows]);
  const shown = useMemo(
    () => sortRows(filterRows(rows, filter), sort, label, fmt.locale),
    [rows, filter, sort, label, fmt.locale]
  );
  const shownTotal = shown.reduce((sum, row) => sum + row.amount, 0);

  const withCount = (text: string, count: number) => (
    <>
      {text} <span className="ml-1 text-ink-4 num">{count}</span>
    </>
  );

  const open = (row: ExpenseRow) => {
    if (busy || row.count === 0) return;
    onOpenTransactions(row);
  };

  const limitCell = (row: ExpenseRow) => {
    const name = label(row);
    if (row.hasLimit) {
      const note = limitNote(row);
      const amount = row.limit?.limit;
      return (
        <>
          <button
            type="button"
            disabled={!limitsReady || busy}
            aria-label={t('actuals.expenses.editLimitFor', { name })}
            onClick={(event) => {
              event.stopPropagation();
              onLimit(row);
            }}
            className="rounded-r1 font-650 text-ink underline decoration-line-strong decoration-dotted underline-offset-[3px] num hover:decoration-ink focus-visible:outline-none focus-visible:shadow-focus disabled:no-underline"
          >
            {amount != null && row.limit?.currency
              ? native(amount, row.limit.currency)
              : tb('actions.editLimit')}
          </button>
          {note?.partial && (
            <small className="mt-0.5 block text-micro font-500 text-ink-4">
              {t('actuals.expenses.partialPeriod')}
            </small>
          )}
        </>
      );
    }
    if (canSetLimit(row)) {
      return (
        <Button
          variant="ghost"
          size="sm"
          className="-mr-2"
          disabled={!limitsReady || busy}
          aria-label={t('actuals.expenses.setLimitFor', { name })}
          onClick={(event) => {
            event.stopPropagation();
            onLimit(row);
          }}
        >
          {tb('actions.setLimit')}
        </Button>
      );
    }
    return <span className="text-ink-4">—</span>;
  };

  const usageCell = (row: ExpenseRow) => {
    const limit = row.limit;
    if (!limit) return <span className="text-micro font-500 text-ink-4">{tb('noBudget')}</span>;
    if (limit.state === 'unavailable' || limit.percentage == null) {
      return <span className="text-micro font-500 text-ink-4">{tb(STATE_KEYS.unavailable)}</span>;
    }
    const over = limit.state === 'over';
    return (
      <div className="min-w-[120px]">
        <div className="mb-[5px] flex items-baseline justify-between gap-2 text-micro font-600">
          <span className={cn(over ? 'text-loss' : 'text-ink-3')}>
            {over && <span aria-hidden>↑ </span>}
            {tb(STATE_KEYS[limit.state])}
          </span>
          <b className={cn('font-650 num', over ? 'text-loss' : 'text-ink-2')}>
            {fmt.percent(limit.percentage / 100, 0)}
          </b>
        </div>
        <Progress
          value={limit.percentage}
          over={over}
          mark={pace}
          className="h-[6px]"
          aria-label={`${tb(STATE_KEYS[limit.state])} · ${fmt.percent(limit.percentage / 100, 0)}`}
        />
        {limit.remaining != null && limit.currency && (
          <small
            className={cn(
              'mt-[5px] text-micro font-600 num',
              NARROW_ONLY,
              limit.remaining < 0 ? 'text-loss' : 'text-ink-3'
            )}
          >
            {limit.remaining < 0
              ? `${native(limit.remaining, limit.currency, { signed: true })} ${t('actuals.expenses.overLimit')}`
              : `${t('actuals.expenses.remaining')} ${native(limit.remaining, limit.currency)}`}
          </small>
        )}
      </div>
    );
  };

  const remainingCell = (row: ExpenseRow) => {
    const limit = row.limit;
    if (!limit) return <span className="text-ink-4">—</span>;
    if (limit.remaining == null || !limit.currency) {
      return (
        <span className="text-ink-4" title={tb('readiness.usageUnavailable')}>
          —<span className="sr-only">{tb('readiness.usageUnavailable')}</span>
        </span>
      );
    }
    if (limit.remaining < 0) {
      return (
        <>
          <b className="block font-650 text-loss">
            {native(limit.remaining, limit.currency, { signed: true })}
          </b>
          <small className="mt-0.5 block text-micro font-500 text-loss">
            {t('actuals.expenses.overLimit')}
          </small>
        </>
      );
    }
    return <b className="font-650 text-ink-2">{native(limit.remaining, limit.currency)}</b>;
  };

  const footer = (left: ReactNode, right?: ReactNode) => (
    <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
      <span className="num">{left}</span>
      {right}
    </div>
  );

  return (
    <Card variant="table">
      <CardHeader className="flex-wrap pb-[15px]">
        <div>
          <CardTitle>{t('actuals.expenses.title')}</CardTitle>
          <CardDescription>{t('actuals.breakdown.sub')}</CardDescription>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" size="sm" onClick={onManageCategories}>
            <Settings2 />
            {t('actuals.expenses.manageCategories')}
          </Button>
          <Button variant="outline" size="sm" onClick={onManageLimits}>
            <SlidersHorizontal />
            {t('actuals.expenses.manageLimits')}
          </Button>
        </div>
      </CardHeader>
      <div className="flex flex-wrap items-center justify-between gap-[10px] px-[21px] pb-[15px]">
        <Segmented
          value={filter}
          onValueChange={setFilter}
          aria-label={t('actuals.expenses.filterLabel')}
          options={[
            { value: 'all', label: withCount(t('actuals.expenses.filterAll'), counts.all) },
            { value: 'over', label: withCount(tb('table.state.overShort'), counts.over) },
            { value: 'none', label: withCount(tb('noBudget'), counts.none) },
          ]}
        />
        <Segmented
          value={sort}
          onValueChange={setSort}
          aria-label={t('actuals.expenses.sortLabel')}
          options={[
            { value: 'amount', label: t('actuals.expenses.sortAmount') },
            { value: 'usage', label: t('actuals.expenses.sortUsage') },
            { value: 'name', label: t('actuals.expenses.sortName') },
          ]}
        />
      </div>
      <div className="overflow-x-auto">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[32%]">{t('actuals.breakdown.category')}</TableHead>
              <TableHead className="text-right">{t('actuals.expenses.spent')}</TableHead>
              <TableHead className="text-right">{t('actuals.expenses.limit')}</TableHead>
              <TableHead className="w-[22%]">{t('actuals.expenses.usage')}</TableHead>
              <TableHead className={cn('text-right', WIDE_ONLY)}>
                {t('actuals.expenses.remaining')}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {shown.length === 0 && (
              <TableRow className="h-[52px] hover:bg-transparent">
                <TableCell colSpan={5} className="text-ink-4">
                  {t('actuals.expenses.emptyFilter')}
                </TableCell>
              </TableRow>
            )}
            {shown.map((row) => {
              const name = label(row);
              return (
                <TableRow
                  key={row.key}
                  className={cn('h-[58px]', row.count > 0 && 'cursor-pointer')}
                  onClick={() => open(row)}
                >
                  <TableCell>
                    <div className="flex items-center gap-3">
                      <CategoryIconChip category={category(row)} />
                      <div className="min-w-0">
                        <button
                          type="button"
                          disabled={busy || row.count === 0}
                          aria-label={t('actuals.expenses.showTransactions', { name })}
                          onClick={(event) => {
                            event.stopPropagation();
                            open(row);
                          }}
                          className="block text-left font-650 text-ink underline-offset-[3px] enabled:hover:underline focus-visible:underline focus-visible:outline-none"
                        >
                          {name}
                        </button>
                        <small className="mt-[3px] block text-micro font-500 text-ink-4 num">
                          <span className="whitespace-nowrap">
                            {t('actuals.transactions', { count: row.count })} ·
                          </span>{' '}
                          <span className="whitespace-nowrap">
                            {t('actuals.expenses.shareOfExpenses', {
                              percent: fmt.percent(row.share, 0),
                            })}
                          </span>
                        </small>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-right font-650 text-ink num">
                    {row.amountUnavailable ? (
                      <span className="text-ink-4" title={t('readiness.fxUnavailable')}>
                        —<span className="sr-only">{t('readiness.fxUnavailable')}</span>
                      </span>
                    ) : (
                      money(row.amount)
                    )}
                  </TableCell>
                  <TableCell className="text-right">{limitCell(row)}</TableCell>
                  <TableCell>{usageCell(row)}</TableCell>
                  <TableCell className={cn('text-right num', WIDE_ONLY)}>
                    {remainingCell(row)}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </div>
      {filter === 'all'
        ? footer(t('actuals.breakdown.total', { amount: money(total) }), evaluationNote)
        : footer(
            t('actuals.expenses.filteredTotal', { amount: money(shownTotal) }),
            t('actuals.expenses.filteredCount', { count: shown.length })
          )}
    </Card>
  );
}
