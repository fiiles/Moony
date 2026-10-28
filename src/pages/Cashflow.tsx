import { AccountScope } from '@/components/reports/AccountScope';
import { PeriodNav } from '@/components/reports/PeriodNav';
import { ReportQualityNote, ReportError } from '@/components/reports/ReportQualityNote';
import {
  ReportTransactionsDialog,
  type ReportTarget,
} from '@/components/reports/ReportTransactionsDialog';
import { useReportMoney } from '@/lib/use-report-money';
import { usePeriodSelection } from '@/hooks/use-period-selection';
import { absoluteMovementShares } from '@/utils/money-flows';
import { expenseRows, type ExpenseRow } from '@/utils/cashflow-budget-rows';
import { budgetOverview, currentMonthStart, limitCoverage } from '@/utils/budget-goal-versions';
import { periodPace } from '@/utils/budget-pace';
import { monthOffsetOf, type PeriodTimeframe } from '@/utils/period';
import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { Link } from 'wouter';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { Trans, useTranslation } from 'react-i18next';
import { Landmark } from 'lucide-react';
import type {
  BudgetGoal,
  BudgetingReport,
  CashflowActuals,
  CashflowGroup,
  TransactionCategory,
} from '@shared/schema';
import { budgetingApi, cashflowApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useShellPage } from '@/components/shell/shell-context';
import { categoryDisplayName } from '@/utils/category-name';
import { useCategories } from '@/hooks/use-categories';
import { CategoryIconChip } from '@/components/common/CategoryIcon';
import { PageHead } from '@/components/shell/PageHead';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Progress } from '@/components/ui/progress';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Button } from '@/components/ui/button';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { EmptyState } from '@/components/common/EmptyState';
import { MoonyBarChart } from '@/components/charts/MoonyBarChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import {
  ExpensesByCategoryCard,
  type LimitNote,
} from '@/components/cashflow/ExpensesByCategoryCard';
import { BudgetGoalDialog } from '@/components/budgeting/BudgetGoalDialog';
import { BudgetManageDialog } from '@/components/budgeting/BudgetManageDialog';
import { CategoryManagerDialog } from '@/components/categories/CategoryManagerDialog';
import { cn } from '@/lib/utils';

interface Row {
  group: CashflowGroup;
  label: string;
  category: TransactionCategory | null;
  amount: number;
  share: number;
}

/** Both reports of one period, account scope and report currency, fetched as one. */
interface Overview {
  actuals: CashflowActuals;
  budgeting: BudgetingReport;
}

interface GoalDialogState {
  open: boolean;
  goal: BudgetGoal | null;
  categoryId: string | null;
  effectiveMonth?: number;
}

const amount = (v: string | number) => Number(v) || 0;

/**
 * Cashflow report with budgets (design system §7 Report, §11; spec
 * `2026-10-28-cashflow-with-budgets-implementation-design.md`): actual income and expenses from
 * bank transactions for a calendar month, quarter or year, the expenses by category with their
 * limits for the period, and income and money transfers (investing, saving, between accounts)
 * apart. Every breakdown row opens its transactions, where they can be recategorized.
 */
export default function Cashflow() {
  const { t } = useTranslation('reports');
  const { t: tBank } = useTranslation('bank_accounts');
  const { t: tBudget } = useTranslation('budgeting');
  const { currencyCode, fxRevision } = useCurrency();
  const [accountIds, setAccountIds] = useState<string[] | undefined>(undefined);
  const [txTarget, setTxTarget] = useState<ReportTarget | null>(null);
  const [goalDialog, setGoalDialog] = useState<GoalDialogState>({
    open: false,
    goal: null,
    categoryId: null,
  });
  const [manageOpen, setManageOpen] = useState(false);
  const [categoriesOpen, setCategoriesOpen] = useState(false);
  const fmt = useFormat();
  const { categories } = useCategories();
  const period = usePeriodSelection('monthly');
  const { range, timeframe, setOffset } = period;

  // Open on the month of the newest transaction, not on an empty current month. Cached success
  // is usable on remount; a failed lookup falls back to the current month. Once a period is
  // picked (here or by the user), a refetch never moves it.
  const {
    data: latestDay,
    isPending: latestPending,
    isFetching: latestFetching,
  } = useQuery({
    queryKey: ['budgeting-latest-transaction'],
    queryFn: () => budgetingApi.getLatestTransactionDate(),
  });
  const [startPicked, setStartPicked] = useState(false);
  if (!latestPending && !latestFetching && !startPicked) {
    setStartPicked(true);
    if (latestDay != null) setOffset(() => monthOffsetOf(latestDay));
  }
  const selection = {
    ...period,
    setTimeframe: (next: PeriodTimeframe) => {
      setStartPicked(true);
      period.setTimeframe(next);
    },
    setOffset: (update: (offset: number) => number) => {
      setStartPicked(true);
      setOffset(update);
    },
  };
  const openMonth = (month: number) => {
    setStartPicked(true);
    period.setTimeframe('monthly');
    setOffset(() => monthOffsetOf(month));
  };

  // One query for both reports, so the expense rows never join a report of the new period or
  // accounts with one of the old. The prefix is the mutation contract's ['cashflow-report'].
  const { data, isLoading, isError, isPlaceholderData, refetch } = useQuery<Overview>({
    queryKey: [
      'cashflow-report',
      'overview',
      range.start,
      range.end,
      timeframe,
      accountIds,
      currencyCode,
      fxRevision,
    ],
    queryFn: async () => {
      const [actuals, budgeting] = await Promise.all([
        cashflowApi.getActuals(range.start, range.end, accountIds, currencyCode),
        budgetingApi.getReport(range.start, range.end, timeframe, accountIds, currencyCode),
      ]);
      return { actuals, budgeting };
    },
    enabled: startPicked,
    staleTime: 0,
    refetchOnMount: 'always',
    placeholderData: keepPreviousData,
  });
  const actuals = data?.actuals;
  const { money, signedMoney, nativeMoney } = useReportMoney(actuals?.reportCurrency);
  // A net refund reads "− 100 Kč" with the minus sign of the signed amounts.
  const lineMoney = (value: number) => (value < 0 ? signedMoney(value) : money(value));

  // Every stored limit, ended ones too: what a limit action edits and what the limit cell notes.
  const {
    data: history,
    isError: historyError,
    refetch: refetchHistory,
  } = useQuery({
    queryKey: ['budget-goal-history'],
    queryFn: () => budgetingApi.getBudgetGoalHistory(),
  });

  useShellPage({
    status: actuals
      ? {
          text: `${tBank('metrics.accounts', { count: actuals.accountCount })} · ${t('actuals.transactions', { count: actuals.transactionCount })} · ${period.labelInSentence}`,
          tone: 'neutral',
        }
      : undefined,
  });

  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);

  const keyLabel = useCallback(
    (key: string, name: string, flowKind: CashflowGroup['flowKind']) => {
      if (key === 'uncategorized') {
        return flowKind === 'transfer'
          ? t('actuals.uncategorizedTransfers')
          : t('actuals.uncategorized');
      }
      return categoryDisplayName(
        categoryById.get(key) ?? { id: key, name, isSystem: key.startsWith('cat_') },
        tBank
      );
    },
    [categoryById, t, tBank]
  );

  // Shares use absolute movement, amounts keep their signs.
  const toRows = useCallback(
    (groups: CashflowGroup[]): Row[] => {
      const kept = groups.filter((g) => g.count > 0 || amount(g.amount) !== 0);
      const shares = absoluteMovementShares(kept.map((g) => amount(g.amount)));
      return kept.map((group, index) => ({
        group,
        label: keyLabel(group.key, group.name, group.flowKind),
        category: categoryById.get(group.key) ?? null,
        amount: amount(group.amount),
        share: shares[index],
      }));
    },
    [keyLabel, categoryById]
  );

  const incomeRows = useMemo(
    () => (actuals ? toRows(actuals.incomeCategories) : []),
    [actuals, toRows]
  );
  const transferRows = useMemo(() => (actuals ? toRows(actuals.transfers) : []), [actuals, toRows]);
  const expenses = useMemo(
    () =>
      data ? expenseRows(data.actuals.expenseCategories, data.budgeting.expenseCategories) : [],
    [data]
  );
  const expenseLabel = useCallback(
    (row: ExpenseRow) => keyLabel(row.key, row.name, 'expense'),
    [keyLabel]
  );

  const nowMonth = currentMonthStart();
  const storedGoal = (row: ExpenseRow): BudgetGoal | null =>
    history?.find(
      (g) => g.categoryId === row.key && g.timeframe === (row.periodGoal?.timeframe ?? 'monthly')
    ) ?? null;

  // A category is budgeted while a limit is in effect or scheduled. A stored goal alone is not
  // enough: it outlives a limit that has run out.
  const availableCategories = useMemo(() => {
    const budgeted = new Set(
      budgetOverview(
        (history ?? []).filter((g) => g.timeframe === 'monthly'),
        currentMonthStart()
      )
        .filter((row) => row.status !== 'ended')
        .map((row) => row.goal.categoryId)
    );
    return categories.filter((c) => c.kind === 'expense' && !budgeted.has(c.id));
  }, [categories, history]);

  // Only what concerns the period shown: earlier or scheduled limits are Spravovat limity's.
  const limitNote = (row: ExpenseRow): LimitNote | null => {
    const goal = storedGoal(row);
    if (!goal) return null;
    const coverage = limitCoverage(goal.versions, range.start, range.end);
    return {
      partial: coverage.covered > 0 && coverage.covered < coverage.months,
    };
  };

  // From a row the limit starts in the month shown. A category that ever had a limit is edited
  // (the form lists what a save replaces); any other expense category gets a new one.
  const openLimit = (row: ExpenseRow) => {
    const goal = storedGoal(row) ?? row.periodGoal;
    setGoalDialog(
      goal
        ? { open: true, goal, categoryId: goal.categoryId, effectiveMonth: range.start }
        : { open: true, goal: null, categoryId: row.key, effectiveMonth: range.start }
    );
  };

  const bars = useMemo(
    () =>
      (actuals?.months ?? []).map((m) => ({
        label: fmt.month(new Date(m.month * 1000), { month: 'short', year: '2-digit' }),
        a: amount(m.income),
        b: amount(m.expenses),
        muted: m.month < actuals!.periodStart || m.month > actuals!.periodEnd,
      })),
    [actuals, fmt]
  );

  // While the next period loads, the rows still belong to the previous one.
  const open = (target: ReportTarget) => {
    if (isPlaceholderData) return;
    setTxTarget(target);
  };
  const openRow = (row: Row) =>
    open({ name: row.label, flowKind: row.group.flowKind, categoryId: row.group.key });

  // The side cards are narrow (about 300 px): tighter inner padding keeps long names in view.
  const side = 'px-[10px] first:pl-[14px] last:pr-[14px]';
  const breakdownTable = (rows: Row[], signed: boolean) => (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead className={cn('w-[44%]', side)}>{t('actuals.breakdown.category')}</TableHead>
          <TableHead className={side}>{t('actuals.breakdown.share')}</TableHead>
          <TableHead className={cn('text-right', side)}>{t('actuals.breakdown.amount')}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.length === 0 && (
          <TableRow className="h-[46px] hover:bg-transparent">
            <TableCell colSpan={3} className="text-ink-4">
              {t('actuals.breakdown.empty')}
            </TableCell>
          </TableRow>
        )}
        {rows.map((row) => (
          <TableRow
            key={row.group.key}
            className="h-[50px] cursor-pointer"
            onClick={() => openRow(row)}
          >
            <TableCell className={cn('font-650 text-ink', side)}>
              <div className="flex items-center gap-[10px]">
                <CategoryIconChip category={row.category} className="shrink-0" />
                <button
                  type="button"
                  className="min-w-0 text-left focus-visible:underline focus-visible:outline-none"
                  onClick={(event) => {
                    event.stopPropagation();
                    openRow(row);
                  }}
                >
                  <span className="block break-words [hyphens:auto]">{row.label}</span>
                  <small className="block text-micro font-500 text-ink-4 num">
                    {t('actuals.transactions', { count: row.group.count })}
                  </small>
                </button>
              </div>
            </TableCell>
            <TableCell className={side}>
              <div className="flex min-w-[56px] items-center gap-2">
                <Progress
                  value={row.share * 100}
                  className="h-[5px] flex-1"
                  aria-label={fmt.percent(row.share, 0)}
                />
                <small className="w-8 text-right text-micro font-600 text-ink-4 num">
                  {fmt.percent(row.share, 0)}
                </small>
              </div>
            </TableCell>
            <TableCell className={cn('whitespace-nowrap text-right font-650 text-ink num', side)}>
              {signed ? signedMoney(row.amount) : lineMoney(row.amount)}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );

  const footer = (left: ReactNode, right?: ReactNode) => (
    <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
      <span className="num">{left}</span>
      {right}
    </div>
  );

  const head = (
    <PageHead
      eyebrow={t('actuals.eyebrow')}
      title={t('actuals.title')}
      description={
        <Trans
          t={t}
          i18nKey="actuals.subtitleLinked"
          components={{
            link: (
              <Link
                href="/reports/cashflow-planning"
                className="font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
              />
            ),
          }}
        />
      }
      actions={
        <>
          <PeriodNav selection={selection} />
          <AccountScope value={accountIds} onChange={setAccountIds} />
        </>
      }
    />
  );

  const dialogs = (
    <>
      <BudgetGoalDialog
        open={goalDialog.open}
        onOpenChange={(next) => setGoalDialog((d) => ({ ...d, open: next }))}
        goal={goalDialog.goal}
        initialCategoryId={goalDialog.categoryId}
        initialEffectiveMonth={goalDialog.effectiveMonth}
        availableCategories={availableCategories}
        categories={categories}
      />
      <BudgetManageDialog
        open={manageOpen}
        onOpenChange={setManageOpen}
        categories={categories}
        canAdd={availableCategories.length > 0}
        onAdd={() => setGoalDialog({ open: true, goal: null, categoryId: null })}
        onEdit={(goal, effectiveMonth) =>
          setGoalDialog({ open: true, goal, categoryId: goal.categoryId, effectiveMonth })
        }
      />
      <CategoryManagerDialog open={categoriesOpen} onOpenChange={setCategoriesOpen} />
    </>
  );

  if (isError && !data)
    return (
      <>
        {head}
        <ReportError onRetry={() => void refetch()} />
        {dialogs}
      </>
    );
  if (isLoading || !data || !actuals) {
    return (
      <>
        {head}
        <Stats columns={3}>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
        {dialogs}
      </>
    );
  }

  // Limits alone keep the report: only a period without transactions and limits is empty.
  if (actuals.transactionCount === 0 && !expenses.some((row) => row.hasLimit)) {
    return (
      <>
        {head}
        <ReportQualityNote quality={actuals.quality} />
        <EmptyState
          icon={<Landmark />}
          title={t('actuals.empty.periodTitle')}
          description={t('actuals.empty.periodDescription')}
          action={
            <Button asChild variant="outline">
              <Link href="/bank-accounts">{t('actuals.empty.action')}</Link>
            </Button>
          }
        />
        {dialogs}
      </>
    );
  }

  const totalIncome = amount(actuals.totalIncome);
  const totalExpenses = amount(actuals.totalExpenses);
  const net = amount(actuals.netBalance);
  const perMonth = (average: string | null) =>
    average != null && actuals.averageCompletedMonths.length >= 2
      ? t('actuals.stats.perMonth', { amount: money(amount(average)) })
      : undefined;
  const pace = periodPace(actuals.periodStart, actuals.periodEnd);
  const evaluated = tBudget('settings.evaluationNote', { period: period.labelInSentence });
  const evaluationNote = pace.isCurrent
    ? `${evaluated} · ${t('actuals.expenses.pace', { percent: fmt.percent(pace.elapsed, 0) })}`
    : evaluated;

  return (
    <>
      {head}
      <div
        className={cn('transition-opacity', isPlaceholderData && 'opacity-60')}
        aria-busy={isPlaceholderData}
      >
        {isError && <ReportError onRetry={() => void refetch()} />}
        {historyError && <ReportError onRetry={() => void refetchHistory()} />}
        <ReportQualityNote quality={actuals.quality} />
        <ReportTransactionsDialog
          target={txTarget}
          start={actuals.periodStart}
          end={actuals.periodEnd}
          accountIds={accountIds}
          reportCurrency={actuals.reportCurrency}
          fxRevision={fxRevision}
          onClose={() => setTxTarget(null)}
        />
        <Stats columns={3}>
          <Stat
            label={t('actuals.stats.income')}
            value={money(totalIncome)}
            note={perMonth(actuals.averageIncome)}
          />
          <Stat
            label={t('actuals.stats.expenses')}
            value={money(totalExpenses)}
            note={perMonth(actuals.averageExpenses)}
          />
          <Stat
            label={t('actuals.stats.savings')}
            value={signedMoney(net)}
            tone={net > 0 ? 'gain' : net < 0 ? 'loss' : 'neutral'}
            noteTone="neutral"
            note={
              totalIncome > 0
                ? t('actuals.stats.savingsRate', { rate: fmt.percent(net / totalIncome, 0) })
                : undefined
            }
          />
        </Stats>

        <Card className="relative mb-7 overflow-hidden px-[21px] pb-4 pt-5 after:pointer-events-none after:absolute after:-right-[100px] after:-top-[120px] after:h-[180px] after:w-[340px] after:rounded-full after:bg-hero-orb after:content-['']">
          <div className="relative z-[1] flex items-center justify-between gap-6">
            <div>
              <h3 className="m-0 text-h3 text-ink">{t('actuals.chart.title')}</h3>
              <p className="mt-[5px] text-micro font-500 text-ink-4">{t('actuals.chart.sub')}</p>
            </div>
            <ChartLegend
              className="mt-0"
              items={[
                { label: t('actuals.chart.income'), swatch: { kind: 'block', color: 'var(--s1)' } },
                {
                  label: t('actuals.chart.expenses'),
                  swatch: { kind: 'block', color: 'var(--s3)' },
                },
              ]}
            />
          </div>
          <MoonyBarChart
            className="relative z-[1] -mx-1 mt-[18px]"
            bars={bars}
            names={[t('actuals.chart.income'), t('actuals.chart.expenses')]}
            height={200}
            formatValue={money}
            renderExtra={(bar) => `${t('actuals.chart.balance')} ${signedMoney(bar.a - bar.b)}`}
            onBarClick={(index) => {
              const month = actuals.months[index]?.month;
              if (month != null && month <= nowMonth) openMonth(month);
            }}
            axisActions={actuals.months.map((m) => ({
              label: t('actuals.chart.openMonth', {
                month: fmt.monthInSentence(new Date(m.month * 1000)),
              }),
              onSelect: () => openMonth(m.month),
              disabled: m.month > nowMonth,
              pressed: timeframe === 'monthly' && m.month === actuals.periodStart,
            }))}
          />
        </Card>

        <div className="grid items-start gap-[18px] min-[1240px]:grid-cols-[minmax(0,1fr)_clamp(300px,24vw,360px)]">
          <ExpensesByCategoryCard
            rows={expenses}
            total={totalExpenses}
            label={expenseLabel}
            category={(row) => categoryById.get(row.key) ?? null}
            canSetLimit={(row) => categoryById.get(row.key)?.kind === 'expense'}
            limitNote={limitNote}
            limitsReady={history != null}
            money={lineMoney}
            native={nativeMoney}
            evaluationNote={evaluationNote}
            pace={pace.isCurrent ? pace.elapsed * 100 : undefined}
            busy={isPlaceholderData}
            onOpenTransactions={(row) =>
              open({ name: expenseLabel(row), flowKind: 'expense', categoryId: row.key })
            }
            onLimit={openLimit}
            onManageCategories={() => setCategoriesOpen(true)}
            onManageLimits={() => setManageOpen(true)}
          />
          <div className="grid items-start gap-[18px] max-[1239px]:grid-cols-2">
            <Card variant="table">
              <CardHeader>
                <div>
                  <CardTitle>{t('actuals.income.title')}</CardTitle>
                  <CardDescription>{t('actuals.breakdown.sub')}</CardDescription>
                </div>
              </CardHeader>
              {breakdownTable(incomeRows, false)}
              {footer(
                t('actuals.breakdown.total', { amount: money(totalIncome) }),
                <span>{t('actuals.income.note')}</span>
              )}
            </Card>
            <Card variant="table">
              <CardHeader>
                <div>
                  <CardTitle>{t('actuals.transfers.title')}</CardTitle>
                  <CardDescription>{t('actuals.transfers.sub')}</CardDescription>
                </div>
              </CardHeader>
              {breakdownTable(transferRows, true)}
              {footer(
                t('actuals.transfers.movement', {
                  amount: signedMoney(amount(actuals.bankCashMovement)),
                })
              )}
            </Card>
          </div>
        </div>
      </div>
      {dialogs}
    </>
  );
}
