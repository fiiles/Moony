import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TransactionCategory } from '@shared/schema';
import {
  filterCounts,
  filterRows,
  sortRows,
  type ExpenseFilter,
  type ExpenseRow,
  type ExpenseSort,
} from '@/utils/cashflow-budget-rows';
import { useFormat } from '@/lib/use-format';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { Segment } from '@/mobile/components/Segment';
import { SortPill } from '@/mobile/components/wealth/SortPill';
import type { LimitMode } from '@/mobile/lib/budget-limit';
import type { RowLimitAction } from '@/mobile/lib/budget-rows';
import { ExpenseCategoryRow } from './ExpenseCategoryRow';
import { SectionFoot } from './SectionFoot';

interface ExpensesSectionProps {
  rows: readonly ExpenseRow[];
  /** The period's expenses in the report currency (the stat above). */
  total: number;
  title: (row: ExpenseRow) => string;
  category: (row: ExpenseRow) => TransactionCategory | null;
  /** The row's transactions, or `undefined` when it has none. */
  href: (row: ExpenseRow) => string | undefined;
  action: (row: ExpenseRow) => RowLimitAction | null;
  partial: (row: ExpenseRow) => boolean;
  money: (amount: number) => string;
  native: (amount: number, currency: string, opts?: { signed?: boolean }) => string;
  /** "Limity vyhodnocené za říjen 2026 · značka je dnešek, uplynulo 23 % období". */
  evaluationNote: string;
  /** How far a running period has got, 0–1. */
  pace?: number;
  /** Spravovat rozpočty, with the tab's period. */
  manageHref: string;
  busy: boolean;
  onLimit: (mode: LimitMode) => void;
}

/**
 * "Výdaje podle kategorií" of the phone Cashflow — the desktop's `ExpensesByCategoryCard` on the
 * phone: the head with "Spravovat limity" (Spravovat rozpočty, where "Výdaje bez limitu" sets new
 * limits), the filter (Vše / Nad limitem / Bez limitu, counted over all rows; hidden while no row
 * has a limit) with the sort pill beside it or wrapped under it, the rows ("Bez limitu" offers
 * "Nastavit limit" on each), and the footer — the total and the evaluation note, or with a filter
 * on the filtered total and how many categories it shows. Filter and sort only change this list.
 */
export function ExpensesSection({
  rows,
  total,
  title,
  category,
  href,
  action,
  partial,
  money,
  native,
  evaluationNote,
  pace,
  manageHref,
  busy,
  onLimit,
}: ExpensesSectionProps) {
  const { t: tr } = useTranslation('reports');
  const { t: tb } = useTranslation('budgeting');
  const fmt = useFormat();
  const [filter, setFilter] = useState<ExpenseFilter>('all');
  const [sort, setSort] = useState<ExpenseSort>('amount');

  const counts = useMemo(() => filterCounts(rows), [rows]);
  // Without any limit every row is "Bez limitu": the filter has nothing to tell apart
  const filterShown = counts.none < counts.all;
  const activeFilter: ExpenseFilter = filterShown ? filter : 'all';
  const shown = useMemo(
    () => sortRows(filterRows(rows, activeFilter), sort, title, fmt.locale),
    [rows, activeFilter, sort, title, fmt.locale]
  );
  const shownTotal = shown.reduce((sum, row) => sum + row.amount, 0);
  const withCount = (text: string, count: number) => (
    <>
      {text} <span className="ml-0.5 text-ink-4 num">{count}</span>
    </>
  );

  return (
    <section>
      <SectionHead
        title={tr('actuals.expenses.title')}
        action={{ label: tr('budgeting:manage.title'), href: manageHref }}
      />
      {rows.length > 0 && (
        // The pill wraps under the filter when both do not fit (360 pt, German, large text)
        <div className="mb-2.5 flex flex-wrap items-center justify-end gap-2">
          {filterShown && (
            <Segment
              className="flex-auto"
              value={filter}
              onChange={setFilter}
              label={tr('actuals.expenses.filterLabel')}
              options={[
                { value: 'all', label: withCount(tr('actuals.expenses.filterAll'), counts.all) },
                { value: 'over', label: withCount(tb('table.state.overShort'), counts.over) },
                { value: 'none', label: withCount(tb('noBudget'), counts.none) },
              ]}
            />
          )}
          <SortPill
            value={sort}
            onChange={setSort}
            label={tr('actuals.expenses.sortLabel')}
            options={[
              { value: 'amount', label: tr('actuals.expenses.sortAmount') },
              { value: 'usage', label: tr('actuals.expenses.sortUsage') },
              { value: 'name', label: tr('actuals.expenses.sortName') },
            ]}
          />
        </div>
      )}
      <EntityRows>
        {shown.length === 0 ? (
          <li className="list-none px-3.5 py-4 text-m-sub text-ink-4">
            {rows.length === 0 ? tr('actuals.breakdown.empty') : tr('actuals.expenses.emptyFilter')}
          </li>
        ) : (
          shown.map((row) => (
            <ExpenseCategoryRow
              key={row.key}
              row={row}
              title={title(row)}
              category={category(row)}
              money={money}
              native={native}
              href={href(row)}
              partial={partial(row)}
              pace={pace}
              action={action(row)}
              offerSetLimit={activeFilter === 'none'}
              busy={busy}
              onLimit={onLimit}
            />
          ))
        )}
      </EntityRows>
      <SectionFoot
        lines={
          activeFilter === 'all'
            ? [tr('actuals.breakdown.total', { amount: money(total) }), evaluationNote]
            : [
                tr('actuals.expenses.filteredTotal', { amount: money(shownTotal) }),
                tr('actuals.expenses.filteredCount', { count: shown.length }),
              ]
        }
      />
    </section>
  );
}
