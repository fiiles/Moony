import { useId, useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useSearch } from 'wouter';
import { Pencil, PiggyBank, Plus, Trash2 } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import { useCategoryName } from '@/hooks/use-categories';
import type { MoneyOptions } from '@/lib/format';
import { useFormat } from '@/lib/use-format';
import { currentMonthStart, type BudgetLimitPeriod } from '@/utils/budget-goal-versions';
import { Button } from '@/components/ui/button';
import { ScaledEmptyState, ScaledReportError } from '@/mobile/components/Scaled';
import { CategoryChip } from '@/mobile/components/cashflow/CategoryChip';
import { RowMenu, RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useBack } from '@/mobile/hooks/use-back';
import { useBudgetLimitSheet } from '@/mobile/hooks/use-budget-limit-sheet';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import {
  budgetRows,
  unbudgetedRows,
  type BudgetRow,
  type UnbudgetedRow,
} from '@/mobile/lib/budget-rows';
import { cashflowSearch, parseCashflowSearch } from '@/mobile/lib/cashflow-period';
import { NavIconButton, NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { BudgetDeleteSheet, type PendingDelete } from '@/mobile/sheets/BudgetDeleteSheet';
import { BudgetLimitSheet } from '@/mobile/sheets/BudgetLimitSheet';

/** Whole amounts ("3 000 Kč"). */
const WHOLE: MoneyOptions = { decimals: 0 };

/**
 * Spravovat rozpočty (spec §8.7, pushed on the Cashflow tab): one row per budget — its limit in
 * effect (or the first scheduled one, or the last of an ended budget), when it is valid and what
 * changes later — then "Výdaje bez limitu", the categories that spend with no limit, each with a
 * "Nastavit limit" button. A tap on a budget, a swipe to the right and the menu's first item edit
 * its limit in the limit sheet; a swipe to the left deletes the budget, the long press's menu also
 * deletes just the interval shown. `+` adds a budget (disabled while every expense category has
 * one). The Cashflow tab's period is only carried so that the way back keeps it.
 */
export default function Budgets() {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('budgeting');
  const fmt = useFormat();
  const categoryName = useCategoryName();
  const search = useSearch();
  const back = useBack(routes.cashflow + cashflowSearch(parseCashflowSearch(search)));
  const limits = useBudgetLimitSheet(true);
  const { categoriesQuery, historyQuery, reportQuery, history } = limits;
  const { categories } = categoriesQuery;
  const [pendingDelete, setPendingDelete] = useState<PendingDelete | null>(null);

  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);
  const names = useMemo(
    () => new Map(categories.map((c) => [c.id, categoryName(c)])),
    [categories, categoryName]
  );
  const rows = useMemo(
    () => budgetRows(history, currentMonthStart(), (id) => names.get(id) ?? id, fmt.locale),
    [history, names, fmt.locale]
  );
  const unbudgeted = useMemo(
    () =>
      unbudgetedRows(
        limits.averages,
        new Map(
          (reportQuery.data?.expenseCategories ?? []).map((c) => [c.categoryId, c.transactionCount])
        ),
        limits.budgeted,
        new Set(categories.filter((c) => c.kind === 'expense').map((c) => c.id))
      ),
    [limits.averages, reportQuery.data, limits.budgeted, categories]
  );

  const addBudget = () => limits.openLimit({ kind: 'add', categoryId: null });
  const loading = historyQuery.isLoading || categoriesQuery.isLoading;
  // The six-month report only feeds the unbudgeted section: its failure stays in that section
  const reportDone = !reportQuery.isLoading && !reportQuery.isError;

  let body: ReactNode;
  if (historyQuery.isError || categoriesQuery.isError) {
    body = (
      <ScaledReportError
        onRetry={() => {
          void historyQuery.refetch();
          void categoriesQuery.refetch();
        }}
      />
    );
  } else if (loading) {
    body = <BudgetsSkeleton />;
  } else if (rows.length === 0 && unbudgeted.length === 0 && reportDone) {
    body = (
      <ScaledEmptyState
        bare
        icon={<PiggyBank />}
        title={tb('noBudgets.title')}
        description={tb('noBudgets.description')}
        className="px-5 py-8"
        action={
          <Button
            className="h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-5 text-m-row"
            disabled={!limits.canPick}
            onClick={addBudget}
          >
            {tb('actions.add')}
          </Button>
        }
      />
    );
  } else {
    body = (
      <>
        {rows.length > 0 && (
          <section>
            <SectionHead
              title={tb('title')}
              note={tb('stats.limited', {
                count: rows.filter((row) => row.status !== 'ended').length,
              })}
            />
            <EntityRows>
              {rows.map((row) => (
                <BudgetItem
                  key={row.key}
                  row={row}
                  category={categoryById.get(row.categoryId) ?? null}
                  // No month: the sheet opens on the current one (the first scheduled limit for a
                  // scheduled budget), so a save changes the limit from now on, never the past
                  onEdit={() => limits.openLimit({ kind: 'edit', goal: row.goal })}
                  onDelete={setPendingDelete}
                />
              ))}
            </EntityRows>
          </section>
        )}
        {(unbudgeted.length > 0 || !reportDone) && (
          <section>
            <SectionHead title={tb('unbudgeted.title')} note={t('cashflow.budgets.averageNote')} />
            {reportQuery.isError ? (
              <ScaledReportError onRetry={() => void reportQuery.refetch()} />
            ) : reportQuery.isLoading ? (
              <BudgetsSkeleton rows={2} />
            ) : (
              <EntityRows>
                {unbudgeted.map((row) => (
                  <UnbudgetedItem
                    key={row.categoryId}
                    row={row}
                    category={categoryById.get(row.categoryId) ?? null}
                    currency={limits.averageCurrency}
                    onSetLimit={() => limits.openLimit({ kind: 'add', categoryId: row.categoryId })}
                  />
                ))}
              </EntityRows>
            )}
          </section>
        )}
      </>
    );
  }

  return (
    <Screen
      title={tb('manage.title')}
      leading={<NavTextButton chevron label={t('tabs.cashflow')} onClick={back} />}
      trailing={
        <NavIconButton
          aria-label={tb('actions.add')}
          disabled={!limits.canPick}
          className="disabled:pointer-events-none disabled:opacity-45"
          onClick={addBudget}
        >
          <Plus strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      }
    >
      <p className="mx-0.5 mb-3.5 text-m-lead text-ink-3">{tb('manage.description')}</p>
      {body}
      <BudgetLimitSheet {...limits.sheetProps} />
      <BudgetDeleteSheet pending={pendingDelete} onClose={() => setPendingDelete(null)} />
    </Screen>
  );
}

interface BudgetItemProps {
  row: BudgetRow;
  category: TransactionCategory | null;
  /** Open the limit sheet on this budget. */
  onEdit: () => void;
  /** Ask to delete the whole budget or the interval shown. */
  onDelete: (pending: PendingDelete) => void;
}

/**
 * One budget as an entity row: category chip and name, the limit as its value, the validity and
 * the changes to come in the sub-line, "Naplánováno" or "ukončen" under the value. An ended
 * budget is dimmed. Tap and swipe right edit, swipe left and the long press's menu delete.
 */
function BudgetItem({ row, category, onEdit, onDelete }: BudgetItemProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const [menuOpen, setMenuOpen] = useState(false);
  const { main, status } = row;

  const monthLabel = (month: number) => fmt.monthInSentence(new Date(month * 1000));
  const limitText = (amount: string | null, currency: string) =>
    moneyText(Number(amount), currency, WHOLE);
  // A running row says only where it starts: what follows is in its sub-line
  const validity = ({ version, end }: BudgetLimitPeriod, running: boolean) => {
    const start = monthLabel(version.effectiveMonth);
    if (end === null || running) return tb('settings.from', { month: start });
    const last = monthLabel(end);
    return start === last ? start : tb('settings.between', { start, end: last });
  };
  const sub = [
    validity(main, status !== 'ended'),
    ...row.upcoming.map((version) =>
      version.amount === null
        ? tb('readiness.scheduledRemoval', { month: monthLabel(version.effectiveMonth) })
        : tb('readiness.scheduledLimit', {
            month: monthLabel(version.effectiveMonth),
            amount: limitText(version.amount, version.currency),
          })
    ),
  ].join(' · ');

  const deleteBudget = () =>
    onDelete({
      kind: 'budget',
      categoryId: row.categoryId,
      timeframe: row.goal.timeframe,
      name: row.name,
    });
  const deleteInterval = () =>
    onDelete({
      kind: 'interval',
      interval: {
        categoryId: row.categoryId,
        timeframe: row.goal.timeframe,
        effectiveMonth: main.version.effectiveMonth,
      },
      name: row.name,
      // The sentence names the limit exactly (the row's value is rounded to whole amounts)
      amount: moneyText(Number(main.version.amount), main.version.currency),
      period: validity(main, false),
    });

  return (
    <EntityRow
      chip={<CategoryChip category={category} />}
      title={row.name}
      sub={sub}
      value={limitText(main.version.amount, main.version.currency)}
      valueSub={
        status === 'scheduled'
          ? tb('manage.scheduled')
          : status === 'ended'
            ? t('cashflow.budgets.ended')
            : undefined
      }
      muted={status === 'ended'}
      onClick={onEdit}
      onSwipeRight={onEdit}
      swipeRightLabel={tb('actions.editLimit')}
      swipeRightIcon={Pencil}
      onSwipeLeft={deleteBudget}
      swipeLeftLabel={tc('buttons.delete')}
      swipeLeftIcon={Trash2}
      onLongPress={() => setMenuOpen(true)}
      accessory={
        <RowMenu
          open={menuOpen}
          onOpenChange={setMenuOpen}
          label={t('cashflow.rowMenu', { title: row.name })}
        >
          <RowMenuItem onSelect={onEdit}>
            <Pencil aria-hidden strokeWidth={1.75} />
            {tb('actions.editLimit')}
          </RowMenuItem>
          {row.canDeleteInterval && (
            <RowMenuItem variant="danger" onSelect={deleteInterval}>
              <Trash2 aria-hidden strokeWidth={1.75} />
              {tb('manage.deleteInterval')}
            </RowMenuItem>
          )}
          <RowMenuItem variant="danger" onSelect={deleteBudget}>
            <Trash2 aria-hidden strokeWidth={1.75} />
            {tb('manage.delete')}
          </RowMenuItem>
        </RowMenu>
      }
    />
  );
}

interface UnbudgetedItemProps {
  row: UnbudgetedRow;
  category: TransactionCategory | null;
  /** The currency of the average (the report's). */
  currency: string;
  onSetLimit: () => void;
}

/**
 * A category that spends and has no limit: the entity row's look (chip, name, sub-line "⌀ 4 120 Kč
 * / měsíc · 31 plateb", divider inset to the name) with "Nastavit limit" as a small secondary
 * button — the row's only action, so the row itself is no tap target (a button inside a button
 * row is invalid). It is laid out like an Údržba row rather than through `EntityRow`'s value slot:
 * that slot clips (`truncate`), which would cut the button's 44 pt hit area down to its 34 pt.
 * The button is named for its row.
 */
function UnbudgetedItem({ row, category, currency, onSetLimit }: UnbudgetedItemProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('budgeting');
  const { t: tr } = useTranslation('reports');
  const categoryName = useCategoryName();
  const moneyText = useMoneyText();
  const titleId = useId();
  const name = category ? categoryName(category) : row.categoryId;
  const sub = [
    tr('actuals.stats.perMonth', { amount: moneyText(row.average, currency, WHOLE) }),
    t('cashflow.moves', { count: row.count }),
  ].join(' · ');

  return (
    <li className="relative flex min-h-[60px] list-none items-center gap-3 px-3.5 py-2.5 before:pointer-events-none before:absolute before:left-[62px] before:right-0 before:top-0 before:border-t before:border-line-soft before:content-[''] first:before:hidden">
      <CategoryChip category={category} />
      <div className="min-w-0 flex-1">
        <div id={titleId} className="truncate text-m-row text-ink">
          {name}
        </div>
        <div className="mt-0.5 truncate text-m-sub text-ink-4">{sub}</div>
      </div>
      <Button
        variant="secondary"
        aria-describedby={titleId}
        className='relative h-auto min-h-[34px] flex-none rounded-[9px] px-[11px] py-1 text-m-sub font-semibold after:absolute after:inset-x-0 after:-inset-y-[5px] after:content-[""]'
        onClick={onSetLimit}
      >
        {tb('actions.setLimit')}
      </Button>
    </li>
  );
}

/** While the budgets load for the first time: rows in outline. */
function BudgetsSkeleton({ rows = 3 }: { rows?: number }) {
  return (
    <div aria-hidden>
      <EntityRows>
        {Array.from({ length: rows }, (_, i) => (
          <li
            key={i}
            className="flex min-h-[60px] list-none items-center gap-3 border-t border-line-soft px-3.5 first:border-t-0"
          >
            <div className="skeleton size-9 flex-none rounded-full" />
            <div className="min-w-0 flex-1">
              <div className="skeleton h-3.5 w-2/5" />
              <div className="skeleton mt-2 h-3 w-3/5" />
            </div>
          </li>
        ))}
      </EntityRows>
    </div>
  );
}
