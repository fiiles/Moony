import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useQuery } from '@tanstack/react-query';
import { useLocation, useSearch } from 'wouter';
import { Landmark } from 'lucide-react';
import type { BudgetGoal, CashflowActuals } from '@shared/schema';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useSpendingPlansOverview } from '@/hooks/use-spending-plans';
import { budgetingApi } from '@/lib/tauri-api';
import { periodPace } from '@/utils/budget-pace';
import { isPartialLimit, storedLimitGoal, type ExpenseRow } from '@/utils/cashflow-budget-rows';
import { yearFromSearch } from '@/utils/spending-plans';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { ScaledEmptyState, ScaledReportError, ScaledReportNote } from '@/mobile/components/Scaled';
import { CashflowBars } from '@/mobile/components/cashflow/CashflowBars';
import { CashflowHero } from '@/mobile/components/cashflow/CashflowHero';
import { CashflowMenu } from '@/mobile/components/cashflow/CashflowMenu';
import { CategoryRow } from '@/mobile/components/cashflow/CategoryRow';
import { ExpensesSection } from '@/mobile/components/cashflow/ExpensesSection';
import { SectionFoot } from '@/mobile/components/cashflow/SectionFoot';
import { PlansCard } from '@/mobile/components/plans/PlansCard';
import { PlansView } from '@/mobile/components/plans/PlansView';
import { UnderlineTabs } from '@/mobile/components/UnderlineTabs';
import { PeriodBar } from '@/mobile/components/cashflow/PeriodBar';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { StatRow } from '@/mobile/components/StatRow';
import { useBudgetLimitSheet } from '@/mobile/hooks/use-budget-limit-sheet';
import { useRemembered } from '@/mobile/hooks/use-remembered';
import { useSummaryMoney } from '@/mobile/hooks/use-summary-money';
import { useCsvExport } from '@/mobile/hooks/use-csv-export';
import {
  useCashflowReport,
  useCashflowState,
  type CashflowReport,
} from '@/mobile/hooks/use-cashflow';
import {
  cashflowSearch,
  isPartialPeriod,
  openingState,
  periodChosen,
  selectMonth,
  type CashflowState,
} from '@/mobile/lib/cashflow-period';
import {
  accountScope,
  barsFor,
  bodyKind,
  categoryRows,
  categoryTitle,
  savingsRate,
  type CategoryRowModel,
} from '@/mobile/lib/cashflow-rows';
import { rowLimitAction } from '@/mobile/lib/budget-rows';
import type { LimitMode } from '@/mobile/lib/budget-limit';
import { cashflowViewSearch, viewFromSearch, type CashflowView } from '@/mobile/lib/cashflow-view';
import { plansCardSummary } from '@/mobile/lib/plan-rows';
import { CASHFLOW_EXPORT_FILE, cashflowCsv } from '@/mobile/lib/cashflow-export';
import { afterHistorySettles } from '@/mobile/lib/history-settle';
import { budgetsPath, cashflowCategoryPath, plansPath, routes } from '@/mobile/shell/routes';
import { LockButton } from '@/mobile/shell/LockButton';
import { Screen } from '@/mobile/shell/Screen';
import { BudgetLimitSheet } from '@/mobile/sheets/BudgetLimitSheet';
import { CashflowAccountsSheet } from '@/mobile/sheets/CashflowAccountsSheet';

/** The card the bars sit in (`.m-card`). */
const CARD =
  'mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 py-3.5 shadow-e1';

/** `Number()` of a money string from the report; a value that cannot be read is 0. */
const amount = (value: string | null): number => Number(value) || 0;

/**
 * Cashflow tab root (spec §8.7, prototype frame 09; the desktop Cashflow is the reference, spec
 * `2026-11-07-mobile-cashflow-parity-design.md`): the period bar, then the report of the selected
 * month, quarter or year — the savings hero, income and expenses, the twelve-month bars (a tap on
 * a month opens it), the expense categories with their limits, income and transfers. The period
 * and the account scope live in the route's query string (`useCashflowState`), so the pages pushed
 * on this tab return to them.
 */
export default function Cashflow() {
  const { t } = useTranslation('mobile');
  const { t: tp } = useTranslation('spending_plans');
  const search = useSearch();
  const [, setLocation] = useLocation();
  const [remembered, setRemembered] = useRemembered<CashflowView>('cashflow.view', 'report');
  const urlView = viewFromSearch(search);
  // The URL decides; a visit that names no view (the tab tapped again) reopens the last one
  const view = urlView ?? remembered;
  const year = yearFromSearch(search, new Date().getFullYear());

  useEffect(() => {
    if (urlView) {
      if (urlView !== remembered) setRemembered(urlView);
    } else if (remembered === 'plans') {
      // Writes the view into the URL, so a plan's detail and the way back return to it
      setLocation(routes.cashflow + cashflowViewSearch(search, 'plans'), { replace: true });
    }
  }, [urlView, remembered, search, setRemembered, setLocation]);

  const switchView = (next: CashflowView) => {
    if (next === view) return;
    setRemembered(next);
    setLocation(routes.cashflow + cashflowViewSearch(search, next), { replace: true });
  };
  const switcher = (
    <UnderlineTabs
      value={view}
      onChange={switchView}
      label={t('tabs.cashflow')}
      options={[
        { value: 'report', label: t('tabs.cashflow') },
        { value: 'plans', label: tp('title') },
      ]}
    />
  );

  return view === 'plans' ? (
    <PlansView
      year={year}
      onYearChange={(next) =>
        setLocation(routes.cashflow + cashflowViewSearch(search, 'plans', next), {
          replace: true,
        })
      }
      switcher={switcher}
      leading={<LockButton />}
    />
  ) : (
    <CashflowReportView switcher={switcher} />
  );
}

/** The report view: the period bar and the report of the selected period. */
function CashflowReportView({ switcher }: { switcher: ReactNode }) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const fmt = useFormat();
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const [state, setState] = useCashflowState();
  const rawSearch = useSearch();
  const chosen = periodChosen(rawSearch);

  // A plain visit opens on the month of the newest transaction (the desktop's rule), decided before
  // the report is fetched: no empty current month first, and the data may be older than the chart.
  // A cached date is used at once; one a money write invalidated is waited for.
  const latestQuery = useQuery({
    queryKey: ['budgeting-latest-transaction'],
    queryFn: () => budgetingApi.getLatestTransactionDate(),
    enabled: !chosen,
    staleTime: Infinity,
  });
  const latest =
    latestQuery.data !== undefined && !(latestQuery.isFetching && latestQuery.isStale)
      ? latestQuery.data
      : latestQuery.isError
        ? null
        : undefined;
  const opening = useMemo(() => openingState(state, chosen, latest), [state, chosen, latest]);
  const current = opening ?? state;
  // Written into the URL once, so a later refetch never moves it and the pushed pages return to it
  useEffect(() => {
    if (!chosen && opening) setState(opening, { pin: true });
  }, [chosen, opening, setState]);

  const report = useCashflowReport(current, { enabled: opening !== null });
  const { run: runExport, isPending: exporting } = useCsvExport();
  const { accounts, isLoading: accountsLoading } = useBankAccounts();
  const [accountsOpen, setAccountsOpen] = useState(false);
  // The accounts chosen in the sheet, waiting for the sheet's history entry to go
  const [pendingScope, setPendingScope] = useState<string[] | null>(null);
  // The six-month report behind the sheet's hint is only fetched once the sheet opens
  const limits = useBudgetLimitSheet(false);
  const { actuals } = report;
  const search = cashflowSearch(current);
  // The plans card is about the current calendar year, whatever period the report shows (M3)
  const planYear = new Date().getFullYear();
  const { data: plansOverview } = useSpendingPlansOverview(planYear);
  const plans = useMemo(
    () => (plansOverview?.year === planYear ? plansCardSummary(plansOverview) : null),
    [plansOverview, planYear]
  );

  const accountCount = accounts.length;
  // The scope as the filter states it (B5): all accounts, or how many are chosen — never the
  // number of accounts that happen to have payments in the period
  const scope = accountScope(current.accountIds, accounts);
  // The invitation is decided by the accounts list, never by the period's own counts (an empty
  // period reports no accounts either)
  const body = bodyKind({
    accountsLoaded: !accountsLoading,
    accountTotal: accountCount,
    actuals,
    isError: report.isError,
    hasLimits: report.expenses.some((row) => row.hasLimit),
  });

  // Export writes the report on screen, never the previous period's still shown while the next
  // loads (the menu item is disabled then); a file the person asked for
  const exportReport = () => {
    if (!actuals || report.isPlaceholder) return;
    const categoryById = new Map(categories.map((category) => [category.id, category]));
    const titleLabels = {
      categoryName,
      uncategorized: tr('actuals.uncategorized'),
      uncategorizedTransfers: tr('actuals.uncategorizedTransfers'),
    };
    void runExport(CASHFLOW_EXPORT_FILE, async () =>
      cashflowCsv(
        actuals,
        (month) => fmt.day(month, { year: 'numeric', month: '2-digit' }),
        (group) =>
          categoryTitle(
            group.key,
            group.flowKind,
            categoryById.get(group.key) ?? null,
            group.name,
            titleLabels
          )
      )
    );
  };

  // The sheet's `history.back()` raises its `popstate` a tick after it closed; the scope is
  // written to the URL (a replace) only after that, else it lands on the sheet's entry and is
  // popped away with it. When that `popstate` never comes (the cleanup found the sheet's history
  // marker already gone), the timer runs it.
  useEffect(() => {
    if (accountsOpen || pendingScope === null) return;
    return afterHistorySettles(window, () => {
      setState({
        ...current,
        accountIds: pendingScope.length === accountCount ? undefined : pendingScope,
      });
      setPendingScope(null);
    });
  }, [accountsOpen, pendingScope, accountCount, current, setState]);

  return (
    <Screen
      title={t('tabs.cashflow')}
      large
      leading={<LockButton />}
      trailing={
        <CashflowMenu
          accountsLabel={tr('readiness.accounts', { count: scope ?? accountCount })}
          onAccounts={() => setAccountsOpen(true)}
          budgetsHref={budgetsPath(search)}
          exportDisabled={exporting || report.isPlaceholder || !actuals}
          onExport={exportReport}
        />
      }
    >
      {switcher}
      <PeriodBar
        timeframe={current.timeframe}
        offset={current.offset}
        label={report.periodLabel}
        pending={opening === null}
        onTimeframeChange={(timeframe) => setState({ ...current, timeframe, offset: 0 })}
        onStep={(delta) => setState({ ...current, offset: Math.min(0, current.offset + delta) })}
        // Whose money it is, under the period (D17): the sentence above it was a row of its own
        scope={
          !accountsLoading && accountCount > 0
            ? {
                label:
                  scope === null ? t('cashflow.scopeAll') : t('cashflow.scope', { count: scope }),
                onOpen: () => setAccountsOpen(true),
              }
            : undefined
        }
      />
      {body === 'error' ? (
        <ScaledReportError onRetry={report.refetch} />
      ) : body === 'skeleton' || !actuals || opening === null ? (
        <CashflowSkeleton />
      ) : body === 'noAccounts' ? (
        <EmptyCashflow />
      ) : (
        // 'emptyPeriod' and 'report': the body itself shows the bars and the empty line
        <div
          className={cn('transition-opacity', report.isPlaceholder && 'opacity-60')}
          aria-busy={report.isPlaceholder}
        >
          {report.isError && <ScaledReportError onRetry={report.refetch} />}
          {limits.historyQuery.isError && (
            <ScaledReportError onRetry={() => void limits.historyQuery.refetch()} />
          )}
          <ReportBody
            actuals={actuals}
            report={report}
            timeframe={current.timeframe}
            onSelectMonth={(month) => setState({ ...current, ...selectMonth(month) })}
            search={search}
            plansCard={plans && <PlansCard summary={plans} href={plansPath(plans.year)} />}
            goals={limits.history}
            limitsReady={limits.historyQuery.data !== undefined}
            onLimit={limits.openLimit}
          />
        </div>
      )}
      <CashflowAccountsSheet
        open={accountsOpen}
        onOpenChange={setAccountsOpen}
        accounts={accounts}
        value={current.accountIds}
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
  timeframe: CashflowState['timeframe'];
  onSelectMonth: (month: number) => void;
  search: string;
  /** The spending plans card (the current year), when it has plans; after the bars. */
  plansCard: ReactNode;
  /** Every stored budget with its versions (the limit sheet edits one; the notes read them). */
  goals: readonly BudgetGoal[];
  /** The stored budgets have loaded, so a limit action knows whether to add or edit. */
  limitsReady: boolean;
  /** Open the limit sheet (an expense row's limit button or menu). */
  onLimit: (mode: LimitMode) => void;
}

/**
 * The report of a period that has transactions or limits (or, with neither, its bars and an empty
 * line). Every amount here is a summary and so whole (`useSummaryMoney`, README §13 "Money and
 * currencies"); the single payments of a category are exact on its own page.
 */
function ReportBody({
  actuals,
  report,
  timeframe,
  onSelectMonth,
  search,
  plansCard,
  goals,
  limitsReady,
  onLimit,
}: ReportBodyProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const { t: tb } = useTranslation('budgeting');
  const fmt = useFormat();
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const { range, expenses, periodLabel } = report;
  const { money, signedMoney, nativeMoney } = useSummaryMoney(actuals.reportCurrency);
  // A net refund reads "− 100 Kč" with the minus sign of the signed amounts
  const lineMoney = (value: number) => (value < 0 ? signedMoney(value) : money(value));

  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);
  const bars = useMemo(
    () => barsFor(actuals.months, actuals.periodStart, actuals.periodEnd),
    [actuals]
  );
  const incomeRows = useMemo(() => categoryRows(actuals.incomeCategories), [actuals]);
  const transferRows = useMemo(() => categoryRows(actuals.transfers), [actuals]);

  const titleLabels = useMemo(
    () => ({
      categoryName,
      uncategorized: tr('actuals.uncategorized'),
      uncategorizedTransfers: tr('actuals.uncategorizedTransfers'),
    }),
    [categoryName, tr]
  );
  const expenseTitle = useCallback(
    (row: ExpenseRow) =>
      categoryTitle(row.key, 'expense', categoryById.get(row.key) ?? null, row.name, titleLabels),
    [categoryById, titleLabels]
  );

  const totalIncome = amount(actuals.totalIncome);
  const totalExpenses = amount(actuals.totalExpenses);
  const net = amount(actuals.netBalance);
  const rate = savingsRate(totalIncome, net);
  // The average per month counts only with two or more completed months, as on the desktop
  const perMonth = (average: string | null) =>
    average !== null && actuals.averageCompletedMonths.length >= 2
      ? tr('actuals.stats.perMonth', { amount: money(amount(average)) })
      : undefined;
  const pace = periodPace(actuals.periodStart, actuals.periodEnd);
  const evaluated = tb('settings.evaluationNote', {
    period: timeframe === 'monthly' ? fmt.monthInSentence(range.startDate) : periodLabel,
  });
  const evaluationNote = pace.isCurrent
    ? `${evaluated} · ${tr('actuals.expenses.pace', { percent: fmt.percent(pace.elapsed, 0) })}`
    : evaluated;

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

  // Limits alone keep the report: only a period without transactions and limits is empty
  if (actuals.transactionCount === 0 && !expenses.some((row) => row.hasLimit)) {
    return (
      <>
        {bars.some((bar) => bar.income !== 0 || bar.expenses !== 0) && barsCard}
        {plansCard}
        <ScaledEmptyState
          bare
          icon={<Landmark />}
          title={tr('actuals.empty.periodTitle')}
          description={tr('actuals.empty.periodDescription')}
          className="px-5 py-8"
        />
      </>
    );
  }

  const rows = (list: CategoryRowModel[], format: (value: number) => string, withBar: boolean) =>
    list.length === 0 ? (
      <li className="list-none px-3.5 py-4 text-m-sub text-ink-4">
        {tr('actuals.breakdown.empty')}
      </li>
    ) : (
      list.map((row) => {
        const category = categoryById.get(row.key) ?? null;
        return (
          <CategoryRow
            key={row.key}
            row={row}
            title={categoryTitle(row.key, row.flowKind, category, row.name, titleLabels)}
            category={category}
            value={format(row.amount)}
            href={
              report.isPlaceholder ? undefined : cashflowCategoryPath(row.flowKind, row.key, search)
            }
            withBar={withBar}
          />
        );
      })
    );

  return (
    <>
      <ScaledReportNote quality={actuals.quality} />
      {isPartialPeriod(range.end, Date.now() / 1000) && (
        <p className="mx-0.5 mb-3 mt-0 text-m-caption text-ink-4">{t('cashflow.partial')}</p>
      )}
      <CashflowHero
        label={tr('actuals.stats.savings')}
        value={net}
        formatValue={signedMoney}
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
      {plansCard}
      <ExpensesSection
        rows={expenses}
        total={totalExpenses}
        title={expenseTitle}
        category={(row) => categoryById.get(row.key) ?? null}
        href={(row) =>
          row.count > 0 ? cashflowCategoryPath('expense', row.key, search) : undefined
        }
        action={(row) =>
          rowLimitAction(
            row,
            categoryById.get(row.key)?.kind === 'expense',
            goals,
            range.start,
            limitsReady
          )
        }
        partial={(row) => {
          const goal = row.hasLimit ? storedLimitGoal(row, goals) : null;
          return goal !== null && isPartialLimit(goal, range.start, range.end);
        }}
        reportCurrency={actuals.reportCurrency}
        money={lineMoney}
        native={nativeMoney}
        evaluationNote={evaluationNote}
        pace={pace.isCurrent ? pace.elapsed : undefined}
        manageHref={budgetsPath(search)}
        busy={report.isPlaceholder}
        onLimit={onLimit}
      />
      <section>
        <SectionHead title={tr('actuals.income.title')} note={money(totalIncome)} />
        <EntityRows>{rows(incomeRows, lineMoney, true)}</EntityRows>
        <SectionFoot lines={[t('cashflow.incomeNote')]} />
      </section>
      <section>
        <SectionHead title={tr('actuals.transfers.title')} />
        <EntityRows>{rows(transferRows, signedMoney, false)}</EntityRows>
        <SectionFoot
          lines={[
            tr('actuals.transfers.sub'),
            tr('actuals.transfers.movement', {
              amount: signedMoney(amount(actuals.bankCashMovement)),
            }),
          ]}
        />
      </section>
    </>
  );
}

/** A profile with no account at all: the invitation to add the first (spec §8.7). */
function EmptyCashflow() {
  const { t } = useTranslation('mobile');
  const [, setLocation] = useLocation();
  return (
    <ScaledEmptyState
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
