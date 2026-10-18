import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { Ellipsis, Landmark } from 'lucide-react';
import type { BudgetGoal, CashflowActuals } from '@shared/schema';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import type { MoneyOptions } from '@/lib/format';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { EmptyState } from '@/components/common/EmptyState';
import { ReportError, ReportQualityNote } from '@/components/reports/ReportQualityNote';
import { BudgetSummaryNote } from '@/mobile/components/cashflow/BudgetSummaryNote';
import { CashflowBars } from '@/mobile/components/cashflow/CashflowBars';
import { CashflowHero } from '@/mobile/components/cashflow/CashflowHero';
import { CategoryRow } from '@/mobile/components/cashflow/CategoryRow';
import { ExpenseCategoryRow } from '@/mobile/components/cashflow/ExpenseCategoryRow';
import { PeriodBar } from '@/mobile/components/cashflow/PeriodBar';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { StatRow } from '@/mobile/components/StatRow';
import { useBudgetLimitSheet } from '@/mobile/hooks/use-budget-limit-sheet';
import {
  useCashflowReport,
  useCashflowState,
  type CashflowReport,
} from '@/mobile/hooks/use-cashflow';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import {
  cashflowSearch,
  isPartialPeriod,
  selectMonth,
  type CashflowState,
} from '@/mobile/lib/cashflow-period';
import {
  barsFor,
  bodyKind,
  budgetCounts,
  categoryRows,
  categoryTitle,
  savingsRate,
  UNCATEGORIZED,
  type CategoryRowModel,
} from '@/mobile/lib/cashflow-rows';
import { reportRowLimit } from '@/mobile/lib/budget-rows';
import { afterHistorySettles } from '@/mobile/lib/history-settle';
import { NavIconButton } from '@/mobile/shell/NavBar';
import { budgetsPath, cashflowCategoryPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { BudgetLimitSheet, type LimitMode } from '@/mobile/sheets/BudgetLimitSheet';
import { CashflowAccountsSheet } from '@/mobile/sheets/CashflowAccountsSheet';

/** Whole amounts, and whole amounts with their sign ("+ 3 120 Kč"). */
const WHOLE: MoneyOptions = { decimals: 0 };
const SIGNED_WHOLE: MoneyOptions = { signed: true, decimals: 0 };

/** The card the bars sit in (`.m-card`). */
const CARD =
  'mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 py-3.5 shadow-e1';

/** `Number()` of a money string from the report; a value that cannot be read is 0. */
const amount = (value: string | null): number => Number(value) || 0;

/**
 * Cashflow tab root (spec §8.7, prototype frame 09): the period bar, then the report of the
 * selected month, quarter or year — the savings hero, income and expenses, the twelve-month bars
 * (a tap on a month opens it), the budget summary line, and the expense, income and transfer
 * categories as rows, each with its share or limit bar. The period and the account scope live in
 * the route's query string (`useCashflowState`), so the pages pushed on this tab return to them.
 */
export default function Cashflow() {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const { t: tb } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  const [, setLocation] = useLocation();
  const [state, setState] = useCashflowState();
  const report = useCashflowReport(state);
  const { accounts, isLoading: accountsLoading } = useBankAccounts();
  const [accountsOpen, setAccountsOpen] = useState(false);
  // The accounts chosen in the sheet, waiting for the sheet's history entry to go
  const [pendingScope, setPendingScope] = useState<string[] | null>(null);
  // The six-month report behind the sheet's hint is only fetched once the sheet opens
  const limits = useBudgetLimitSheet(false);
  const { actuals } = report;
  const search = cashflowSearch(state);
  const accountCount = accounts.length;
  // The invitation is decided by the accounts list, never by the period's own counts (an empty
  // period reports no accounts either)
  const body = bodyKind({
    accountsLoaded: !accountsLoading,
    accountTotal: accountCount,
    actuals,
    isError: report.isError,
  });

  // The sheet's `history.back()` raises its `popstate` a tick after it closed; the scope is
  // written to the URL (a replace) only after that, else it lands on the sheet's entry and is
  // popped away with it. When that `popstate` never comes (the cleanup found the sheet's history
  // marker already gone), the timer runs it.
  useEffect(() => {
    if (accountsOpen || pendingScope === null) return;
    return afterHistorySettles(window, () => {
      setState({
        ...state,
        accountIds: pendingScope.length === accountCount ? undefined : pendingScope,
      });
      setPendingScope(null);
    });
  }, [accountsOpen, pendingScope, accountCount, state, setState]);

  return (
    <Screen
      title={t('tabs.cashflow')}
      large
      subtitle={
        actuals && actuals.accountCount > 0
          ? t('cashflow.subtitle', { count: actuals.accountCount })
          : undefined
      }
      trailing={
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <NavIconButton aria-label={tc('labels.moreActions')}>
              <Ellipsis strokeWidth={1.75} aria-hidden />
            </NavIconButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-[220px]">
            <DropdownMenuItem
              className="min-h-11 text-m-body"
              onSelect={() => setAccountsOpen(true)}
            >
              {tr('readiness.accounts', { count: state.accountIds?.length ?? accountCount })}
            </DropdownMenuItem>
            <DropdownMenuItem
              className="min-h-11 text-m-body"
              onSelect={() => setLocation(budgetsPath(search))}
            >
              {tb('manage.title')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      }
    >
      <PeriodBar
        timeframe={state.timeframe}
        offset={state.offset}
        label={report.periodLabel}
        onTimeframeChange={(timeframe) => setState({ ...state, timeframe, offset: 0 })}
        onStep={(delta) => setState({ ...state, offset: Math.min(0, state.offset + delta) })}
      />
      {body === 'error' ? (
        <ReportError onRetry={report.refetch} />
      ) : body === 'skeleton' || !actuals ? (
        <CashflowSkeleton />
      ) : body === 'noAccounts' ? (
        <EmptyCashflow />
      ) : (
        // 'emptyPeriod' and 'report': the body itself shows the bars and the empty line
        <div
          className={cn('transition-opacity', report.isPlaceholder && 'opacity-60')}
          aria-busy={report.isPlaceholder}
        >
          {report.isError && <ReportError onRetry={report.refetch} />}
          <ReportBody
            actuals={actuals}
            report={report}
            state={state}
            onSelectMonth={(month) => setState({ ...state, ...selectMonth(month) })}
            search={search}
            goals={limits.history}
            onLimit={limits.openLimit}
          />
        </div>
      )}
      <CashflowAccountsSheet
        open={accountsOpen}
        onOpenChange={setAccountsOpen}
        accounts={accounts}
        value={state.accountIds}
        onDone={(ids) => {
          setPendingScope(ids);
          setAccountsOpen(false);
        }}
      />
      <BudgetLimitSheet {...limits.sheetProps} />
    </Screen>
  );
}

interface ReportBodyProps {
  actuals: CashflowActuals;
  report: CashflowReport;
  state: CashflowState;
  onSelectMonth: (month: number) => void;
  search: string;
  /** Every stored budget with its versions (the edit sheet starts from one). */
  goals: readonly BudgetGoal[];
  /** Open the limit sheet (an expense row's menu). */
  onLimit: (mode: LimitMode) => void;
}

/** The report of a period that has transactions (or, with none, its bars and an empty line). */
function ReportBody({
  actuals,
  report,
  state,
  onSelectMonth,
  search,
  goals,
  onLimit,
}: ReportBodyProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const { reportCurrency, range, budgeting, budgetByCategory } = report;

  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);
  const bars = useMemo(
    () => barsFor(actuals.months, actuals.periodStart, actuals.periodEnd),
    [actuals]
  );
  const counts = useMemo(() => budgetCounts(budgeting?.expenseCategories ?? []), [budgeting]);
  // The budgets' summaries are the expense categories': income and transfers get no limits
  const expenseRows = useMemo(
    () => categoryRows(actuals.expenseCategories, budgetByCategory),
    [actuals, budgetByCategory]
  );
  const incomeRows = useMemo(() => categoryRows(actuals.incomeCategories), [actuals]);
  const transferRows = useMemo(() => categoryRows(actuals.transfers), [actuals]);

  const titleLabels = {
    categoryName,
    uncategorized: tr('actuals.uncategorized'),
    uncategorizedTransfers: tr('actuals.uncategorizedTransfers'),
  };

  const totalIncome = amount(actuals.totalIncome);
  const totalExpenses = amount(actuals.totalExpenses);
  const net = amount(actuals.netBalance);
  const rate = savingsRate(totalIncome, net);
  const money = (value: number) => moneyText(value, reportCurrency, WHOLE);
  // The average per month counts only with two or more completed months, as on the desktop
  const perMonth = (average: string | null) =>
    average !== null && actuals.averageCompletedMonths.length >= 2
      ? tr('actuals.stats.perMonth', { amount: money(amount(average)) })
      : undefined;

  const barsCard = (
    <div className={CARD}>
      <CashflowBars
        bars={bars}
        names={[tr('actuals.chart.income'), tr('actuals.chart.expenses')]}
        label={tr('actuals.chart.title')}
        monthLabel={(month) => fmt.month(new Date(month * 1000))}
        onSelectMonth={onSelectMonth}
      />
    </div>
  );

  if (actuals.transactionCount === 0) {
    return (
      <>
        {bars.some((bar) => bar.income !== 0 || bar.expenses !== 0) && barsCard}
        <EmptyState
          bare
          icon={<Landmark />}
          title={tr('actuals.empty.periodTitle')}
          description={tr('actuals.empty.periodDescription')}
          className="px-5 py-8"
        />
      </>
    );
  }

  // The expense rows carry the long press's menu (the limit sheet, the drilldown)
  const rows = (list: CategoryRowModel[], withBar: boolean, withMenu = false) =>
    list.length === 0 ? (
      <li className="list-none px-3.5 py-4 text-m-sub text-ink-4">
        {tr('actuals.breakdown.empty')}
      </li>
    ) : (
      list.map((row) => {
        const category = categoryById.get(row.key) ?? null;
        const title = categoryTitle(row.key, row.flowKind, category, row.name, titleLabels);
        const href = cashflowCategoryPath(row.flowKind, row.key, search);
        if (!withMenu || row.key === UNCATEGORIZED) {
          return (
            <CategoryRow
              key={row.key}
              row={row}
              title={title}
              category={category}
              reportCurrency={reportCurrency}
              href={href}
              withBar={withBar}
            />
          );
        }
        const { hasLimit, mode } = reportRowLimit(
          row.key,
          budgetByCategory.get(row.key)?.budgetGoal,
          goals,
          range.start,
          report.budgeting !== undefined
        );
        return (
          <ExpenseCategoryRow
            key={row.key}
            row={row}
            title={title}
            category={category}
            reportCurrency={reportCurrency}
            href={href}
            withBar={withBar}
            hasLimit={hasLimit}
            onLimit={mode && (() => onLimit(mode))}
          />
        );
      })
    );

  return (
    <>
      <ReportQualityNote quality={actuals.quality} />
      {isPartialPeriod(range.end, Date.now() / 1000) && (
        <p className="mx-0.5 mb-3 mt-0 text-m-caption text-ink-4">{t('cashflow.partial')}</p>
      )}
      <CashflowHero
        label={tr('actuals.stats.savings')}
        value={net}
        formatValue={(value) => moneyText(value, reportCurrency, SIGNED_WHOLE)}
        tone={net > 0 ? 'gain' : net < 0 ? 'loss' : 'neutral'}
        note={
          rate !== null ? tr('actuals.stats.savingsRate', { rate: fmt.percent(rate, 0) }) : null
        }
      />
      <StatRow
        stats={[
          {
            label: tr('actuals.stats.income'),
            value: money(totalIncome),
            note: perMonth(actuals.averageIncome),
          },
          {
            label: tr('actuals.stats.expenses'),
            value: money(totalExpenses),
            note: perMonth(actuals.averageExpenses),
          },
        ]}
      />
      {barsCard}
      {state.timeframe === 'monthly' && counts && (
        <BudgetSummaryNote counts={counts} href={budgetsPath(search)} />
      )}
      <section>
        <SectionHead title={tr('actuals.expenses.title')} note={money(totalExpenses)} />
        <EntityRows>{rows(expenseRows, true, true)}</EntityRows>
      </section>
      <section>
        <SectionHead title={tr('actuals.income.title')} note={money(totalIncome)} />
        <EntityRows>{rows(incomeRows, true)}</EntityRows>
      </section>
      {transferRows.length > 0 && (
        <section>
          <SectionHead title={tr('actuals.transfers.title')} />
          <EntityRows>{rows(transferRows, false)}</EntityRows>
        </section>
      )}
    </>
  );
}

/** A profile with no account at all: the invitation to add the first (spec §8.7). */
function EmptyCashflow() {
  const { t } = useTranslation('mobile');
  const [, setLocation] = useLocation();
  return (
    <EmptyState
      icon={<Landmark />}
      title={t('cashflow.empty.title')}
      description={t('cashflow.empty.description')}
      className="px-5 py-10"
      action={
        <Button
          className="h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-3 text-m-row"
          onClick={() => setLocation(routes.finance)}
        >
          {t('cashflow.empty.action')}
        </Button>
      }
    />
  );
}

/** While the report loads for the first time: the hero, the two stats and the bars in outline. */
function CashflowSkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 rounded-r4 border border-line bg-paper px-4 pb-3 pt-4 shadow-e1">
        <div className="skeleton h-3 w-20" />
        <div className="skeleton mt-3 h-9 w-48" />
        <div className="skeleton mt-3 h-3 w-32" />
      </div>
      <div className="mb-3.5 grid grid-cols-2 gap-2.5">
        {[0, 1].map((i) => (
          <div key={i} className="rounded-r3 border border-line bg-paper px-3 py-2.5 shadow-e1">
            <div className="skeleton h-2.5 w-3/5" />
            <div className="skeleton mt-2.5 h-4 w-4/5" />
          </div>
        ))}
      </div>
      <div className={CARD}>
        <div className="skeleton h-[110px] w-full" />
      </div>
    </div>
  );
}
