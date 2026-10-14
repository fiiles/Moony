import { AccountScope } from '@/components/reports/AccountScope';
import { ReportQualityNote, ReportError } from '@/components/reports/ReportQualityNote';
import { useReportMoney } from '@/lib/use-report-money';
import { useCurrency } from '@/lib/currency';
import { summarizeBudgetStatus } from '@/utils/report-currencies';
import { useMemo, useState } from 'react';
import { Link } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Ellipsis, Landmark, PiggyBank, SlidersHorizontal } from 'lucide-react';
import { budgetingApi, type BudgetGoal } from '@/lib/tauri-api';
import type { TransactionCategory } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useShellPage } from '@/components/shell/shell-context';
import { usePeriodSelection } from '@/hooks/use-period-selection';
import { monthOffsetOf } from '@/utils/period';
import { PeriodNav } from '@/components/reports/PeriodNav';
import type { BudgetState } from '@/utils/budget-pace';
import { budgetOverview, currentMonthStart, scheduledVersions } from '@/utils/budget-goal-versions';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Segmented } from '@/components/ui/segmented';
import { Progress } from '@/components/ui/progress';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
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
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { EmptyState } from '@/components/common/EmptyState';
import { CategoryIconChip } from '@/components/common/CategoryIcon';
import { BudgetGoalDialog } from '@/components/budgeting/BudgetGoalDialog';
import { BudgetManageDialog } from '@/components/budgeting/BudgetManageDialog';
import {
  CategoryTransactionsDialog,
  type CategoryTransactionsTarget,
} from '@/components/budgeting/CategoryTransactionsDialog';

type SortKey = 'usage' | 'name';

interface BudgetRow {
  category: TransactionCategory | null;
  goal: BudgetGoal | null;
  categoryId: string;
  name: string;
  budgetCurrency: string | null;
  budgetSpent: string | null;
  budgetLimitNative: string | null;
  budgetPercentage: number | null;
  count: number;
  state: BudgetState | null;
}

interface UnbudgetedRow {
  category: TransactionCategory | null;
  categoryId: string;
  name: string;
  spent: number;
  count: number;
  share: number;
  canBudget: boolean;
}

/**
 * Budgets (design system §7 Planning, prototype `budgets.html`): period
 * navigation, report-currency spending and native-currency budget states,
 * one table of limited categories with state badges, and
 * the spending that has no limit yet.
 */
export default function Budgeting() {
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  const { currencyCode, fxRevision } = useCurrency();
  const [accountIds, setAccountIds] = useState<string[] | undefined>(undefined);
  const fmt = useFormat();
  const categoryName = useCategoryName();
  const [manageOpen, setManageOpen] = useState(false);

  const period = usePeriodSelection('monthly');
  const { timeframe, range, setOffset } = period;
  // Open on the month of the newest transaction, not on an empty current month.
  const {
    data: latestDay,
    isPending: latestPending,
    isFetching: latestFetching,
  } = useQuery({
    queryKey: ['budgeting-latest-transaction'],
    queryFn: () => budgetingApi.getLatestTransactionDate(),
  });
  const [startPicked, setStartPicked] = useState(false);
  // Cached success is already usable on remount; a failed lookup falls back
  // to the current month. Waiting for a fetch *after* mount deadlocks fresh cache.
  if (!latestPending && !latestFetching && !startPicked) {
    setStartPicked(true);
    if (latestDay != null) setOffset(() => monthOffsetOf(latestDay));
  }
  const [sort, setSort] = useState<SortKey>('usage');
  const [goalDialog, setGoalDialog] = useState<{
    open: boolean;
    goal: BudgetGoal | null;
    categoryId: string | null;
    effectiveMonth?: number;
  }>({ open: false, goal: null, categoryId: null });
  const [txTarget, setTxTarget] = useState<CategoryTransactionsTarget | null>(null);

  const {
    data: report,
    isLoading: reportLoading,
    isError: reportError,
    refetch: refetchReport,
  } = useQuery({
    queryKey: [
      'budgeting-report',
      range.start,
      range.end,
      timeframe,
      accountIds,
      currencyCode,
      fxRevision,
    ],
    queryFn: () =>
      budgetingApi.getReport(range.start, range.end, timeframe, accountIds, currencyCode),
    enabled: startPicked,
  });
  const { money, nativeMoney } = useReportMoney(report?.reportCurrency);
  const {
    data: goals = [],
    isLoading: goalsLoading,
    isError: goalsError,
    refetch: refetchGoals,
  } = useQuery({
    queryKey: ['budget-goals'],
    queryFn: () => budgetingApi.getBudgetGoals(),
  });
  const {
    categories,
    isLoading: categoriesLoading,
    isError: categoriesError,
    refetch: refetchCategories,
  } = useCategories();
  const { accounts } = useBankAccounts();

  // The server has already resolved explicit overrides, own identity and category roles.
  const spentByCategory = useMemo(() => {
    const map = new Map<string, { spent: number; count: number }>();
    for (const c of report?.expenseCategories ?? []) {
      map.set(c.categoryId, {
        spent: parseFloat(c.totalAmount) || 0,
        count: c.transactionCount,
      });
    }
    return map;
  }, [report]);

  const rows = useMemo<BudgetRow[]>(() => {
    const list = (report?.expenseCategories ?? [])
      .filter((c) => c.budgetGoal)
      .map((historical) => {
        const categoryId = historical.categoryId;
        const goal =
          goals.find(
            (g) => g.categoryId === categoryId && g.timeframe === historical.budgetGoal?.timeframe
          ) ?? null;
        const category = categories.find((c) => c.id === categoryId);
        const usage = spentByCategory.get(categoryId);
        const budgetPercentage = historical?.budgetPercentage ?? null;
        const state: BudgetState | null =
          budgetPercentage == null ||
          !Number.isFinite(budgetPercentage) ||
          !historical?.budgetCurrency ||
          historical?.budgetSpent == null ||
          historical?.budgetLimitNative == null
            ? null
            : budgetPercentage > 100
              ? 'over'
              : budgetPercentage >= 85
                ? 'near'
                : 'ok';
        return {
          goal,
          category: category ?? null,
          categoryId,
          name: category ? categoryName(category) : categoryId,
          budgetCurrency: historical?.budgetCurrency ?? null,
          budgetSpent: historical?.budgetSpent ?? null,
          budgetLimitNative: historical?.budgetLimitNative ?? null,
          budgetPercentage,
          count: usage?.count ?? 0,
          state,
        };
      });
    return list.sort((a, b) =>
      sort === 'name'
        ? a.name.localeCompare(b.name, fmt.locale)
        : (b.budgetPercentage ?? -Infinity) - (a.budgetPercentage ?? -Infinity)
    );
  }, [goals, report, categories, categoryName, spentByCategory, sort, fmt.locale]);

  const unbudgeted = useMemo<UnbudgetedRow[]>(() => {
    const budgeted = new Set(rows.map((r) => r.categoryId));
    const list: UnbudgetedRow[] = [];
    for (const [categoryId, usage] of spentByCategory) {
      if (budgeted.has(categoryId) || (usage.spent === 0 && usage.count === 0)) continue;
      const category = categories.find((c) => c.id === categoryId);
      list.push({
        category: category ?? null,
        categoryId,
        name: category ? categoryName(category) : categoryId,
        spent: usage.spent,
        count: usage.count,
        share: 0,
        canBudget: category?.kind === 'expense',
      });
    }
    const uncategorized = parseFloat(report?.uncategorizedExpenses ?? '0') || 0;
    const uncategorizedCount = report?.uncategorizedExpenseTransactionCount ?? 0;
    if (uncategorized !== 0 || uncategorizedCount > 0) {
      list.push({
        category: null,
        categoryId: 'uncategorized',
        name: t('unbudgeted.uncategorized'),
        spent: uncategorized,
        count: uncategorizedCount,
        share: 0,
        canBudget: false,
      });
    }
    const movement = list.reduce((sum, row) => sum + Math.abs(row.spent), 0);
    for (const row of list) row.share = movement ? Math.abs(row.spent) / movement : 0;
    return list.sort((a, b) => b.spent - a.spent);
  }, [rows, spentByCategory, categories, categoryName, report, t]);

  // A category is budgeted while a limit is in effect or scheduled. A stored
  // goal row alone is not enough: it outlives a limit that has run out.
  const availableCategories = useMemo(() => {
    const budgeted = new Set(
      budgetOverview(
        goals.filter((g) => g.timeframe === 'monthly'),
        currentMonthStart()
      )
        .filter((row) => row.status !== 'ended')
        .map((row) => row.goal.categoryId)
    );
    return categories.filter((c) => c.kind === 'expense' && !budgeted.has(c.id));
  }, [categories, goals]);

  const status = summarizeBudgetStatus(rows, report?.totalExpenses ?? '0');

  const periodInSentence = period.labelInSentence;

  useShellPage({
    status: report
      ? {
          text: t('status', {
            count: accounts.length,
            period: periodInSentence,
          }),
          tone: 'neutral',
        }
      : undefined,
  });

  // From the report: the limit starts in the month shown; from the manager: this month.
  const openAdd = (categoryId: string | null = null, effectiveMonth?: number) =>
    setGoalDialog({ open: true, goal: null, categoryId, effectiveMonth });
  const openEdit = (row: BudgetRow) =>
    setGoalDialog({
      open: true,
      goal: row.goal,
      categoryId: row.categoryId,
      effectiveMonth: range.start,
    });

  const stateBadge = (row: BudgetRow) => {
    if (
      row.state == null ||
      !row.budgetCurrency ||
      row.budgetSpent == null ||
      row.budgetLimitNative == null
    )
      return <Badge variant="outline">{t('readiness.usageUnavailable')}</Badge>;
    if (row.state === 'over') {
      return (
        <Badge variant="loss">
          {t('table.state.over', {
            amount: nativeMoney(
              Number(row.budgetSpent) - Number(row.budgetLimitNative),
              row.budgetCurrency
            ),
          })}
        </Badge>
      );
    }
    if (row.state === 'near') return <Badge>{t('table.state.near')}</Badge>;
    return <Badge variant="gain">{t('table.state.ok')}</Badge>;
  };

  const countLine = (count: number) =>
    count > 0 ? t('table.transactions', { count }) : t('table.noTransactions');

  const openTransactions = (row: BudgetRow) =>
    setTxTarget({
      categoryId: row.categoryId,
      name: row.name,
      budgetCurrency: row.budgetCurrency ?? row.goal?.currency ?? undefined,
    });

  const head = (
    <>
      <PageHead
        eyebrow={t('eyebrow')}
        title={t('title')}
        description={t('subtitle')}
        actions={
          <>
            <PeriodNav
              selection={{
                ...period,
                setTimeframe: (next) => {
                  setStartPicked(true);
                  period.setTimeframe(next);
                },
                setOffset: (update) => {
                  setStartPicked(true);
                  setOffset(update);
                },
              }}
            />
            <AccountScope value={accountIds} onChange={setAccountIds} />
            <Button onClick={() => setManageOpen(true)}>
              <SlidersHorizontal />
              {t('manage.title')}
            </Button>
          </>
        }
      />
    </>
  );

  const dialogs = (
    <>
      <BudgetGoalDialog
        open={goalDialog.open}
        onOpenChange={(open) => setGoalDialog((d) => ({ ...d, open }))}
        goal={goalDialog.goal}
        initialCategoryId={goalDialog.categoryId}
        initialEffectiveMonth={goalDialog.effectiveMonth}
        availableCategories={availableCategories}
        categories={categories}
      />
      <CategoryTransactionsDialog
        accountIds={accountIds}
        target={txTarget}
        reportCurrency={txTarget?.budgetCurrency ?? report?.reportCurrency ?? currencyCode}
        fxRevision={fxRevision}
        onClose={() => setTxTarget(null)}
        start={range.start}
        end={range.end}
        periodLabel={period.label}
      />
      <BudgetManageDialog
        open={manageOpen}
        onOpenChange={setManageOpen}
        categories={categories}
        canAdd={availableCategories.length > 0}
        onAdd={() => openAdd()}
        onEdit={(goal, effectiveMonth) =>
          setGoalDialog({ open: true, goal, categoryId: goal.categoryId, effectiveMonth })
        }
      />
    </>
  );

  if (reportError || goalsError || categoriesError)
    return (
      <>
        {head}
        <ReportError
          onRetry={() => {
            void refetchReport();
            void refetchGoals();
            void refetchCategories();
          }}
        />
        {dialogs}
      </>
    );
  if (reportLoading || goalsLoading || categoriesLoading || !report) {
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

  const noTransactions =
    report.incomeCategories.length === 0 &&
    report.expenseCategories.length === 0 &&
    report.uncategorizedTransactionCount === 0 &&
    report.quality.missingFxCount === 0;

  // nothing booked in the period and no limits to show
  if (rows.length === 0 && noTransactions) {
    return (
      <>
        {head}
        <ReportQualityNote quality={report.quality} />
        <EmptyState
          icon={<Landmark />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <Button asChild>
              <Link href="/bank-accounts">{t('empty.cta')}</Link>
            </Button>
          }
        />
        {dialogs}
      </>
    );
  }

  const unbudgetedTotal = unbudgeted.reduce((s, r) => s + r.spent, 0);

  const unbudgetedCard = unbudgeted.length > 0 && (
    <Card variant="table" className="mt-[18px]">
      <CardHeader>
        <div>
          <CardTitle>{t('unbudgeted.title')}</CardTitle>
          <CardDescription>{t('unbudgeted.sub')}</CardDescription>
        </div>
      </CardHeader>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-[30%]">{t('table.category')}</TableHead>
            <TableHead>{t('readiness.absoluteShare')}</TableHead>
            <TableHead className="text-right">{t('unbudgeted.spent')}</TableHead>
            <TableHead className="w-[150px]" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {unbudgeted.map((row) => (
            <TableRow
              key={row.categoryId}
              className="h-[52px] cursor-pointer"
              onClick={() => setTxTarget({ categoryId: row.categoryId, name: row.name })}
            >
              <TableCell>
                <div className="flex items-center gap-3">
                  <CategoryIconChip category={row.category} />
                  <div className="min-w-0">
                    <button
                      type="button"
                      className="block text-left font-650 text-ink underline-offset-[3px] hover:underline focus-visible:outline-none focus-visible:underline"
                    >
                      {row.name}
                    </button>
                    <small className="mt-[3px] block text-micro font-500 text-ink-4">
                      {countLine(row.count)}
                    </small>
                  </div>
                </div>
              </TableCell>
              <TableCell>
                <div className="flex items-center gap-[10px] pr-6">
                  <Progress
                    value={row.share * 100}
                    className="h-[5px] flex-1"
                    aria-label={fmt.percent(row.share, 0)}
                  />
                  <small className="w-9 text-right text-micro font-650 text-ink-4 num">
                    {fmt.percent(row.share, 0)}
                  </small>
                </div>
              </TableCell>
              <TableCell className="text-right font-650 text-ink num">{money(row.spent)}</TableCell>
              <TableCell className="text-right">
                {row.canBudget && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={(e) => {
                      e.stopPropagation();
                      openAdd(row.categoryId, range.start);
                    }}
                  >
                    {t('actions.setLimit')}
                  </Button>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
        <span className="num">
          {t('readiness.signedUnbudgetedTotal', { amount: money(unbudgetedTotal) })}
        </span>
      </div>
    </Card>
  );

  if (rows.length === 0) {
    return (
      <>
        {head}
        <ReportQualityNote quality={report.quality} />
        <EmptyState
          icon={<PiggyBank />}
          title={t('settings.noPeriodLimits')}
          description={t('settings.noPeriodLimitsHelp')}
          action={
            <Button onClick={() => setManageOpen(true)}>
              <SlidersHorizontal />
              {t('manage.title')}
            </Button>
          }
        />
        {unbudgetedCard}
        {dialogs}
      </>
    );
  }

  return (
    <>
      {head}
      {report && <ReportQualityNote quality={report.quality} />}
      <Stats columns={3}>
        <Stat
          label={t('stats.totalExpenses')}
          value={money(Number(status.targetExpenses))}
          note={t('stats.limited', { count: rows.length })}
        />
        <Stat
          label={t('stats.onTrack')}
          value={String(status.onTrack)}
          note={t('stats.nearCount', { count: status.near })}
        />
        <Stat
          label={t('stats.overCount')}
          value={String(status.over)}
          tone={status.over > 0 ? 'loss' : 'neutral'}
          note={t('stats.unavailableCount', { count: status.unavailable })}
        />
      </Stats>

      <Card variant="table">
        <CardHeader>
          <div>
            <CardTitle>{t('table.title')}</CardTitle>
            <CardDescription>
              {sort === 'usage' ? t('table.sortedByUsage') : t('table.sortedByName')}
            </CardDescription>
          </div>
          <Segmented
            value={sort}
            onValueChange={setSort}
            options={[
              { value: 'usage', label: t('table.sort.usage') },
              { value: 'name', label: t('table.sort.name') },
            ]}
          />
        </CardHeader>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[26%]">{t('table.category')}</TableHead>
              <TableHead>{t('table.usage')}</TableHead>
              <TableHead className="w-[18%] text-right">{t('table.spentOfLimit')}</TableHead>
              <TableHead className="w-[20%] text-right">{t('table.stateHead')}</TableHead>
              <TableHead className="w-10" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow
                key={row.categoryId}
                className="h-[60px] cursor-pointer"
                onClick={() => openTransactions(row)}
              >
                <TableCell>
                  <div className="flex items-center gap-3">
                    <CategoryIconChip category={row.category} />
                    <div className="min-w-0">
                      <button
                        type="button"
                        className="block text-left font-650 text-ink underline-offset-[3px] hover:underline focus-visible:outline-none focus-visible:underline"
                      >
                        {row.name}
                      </button>
                      <small className="mt-[3px] block text-micro font-500 text-ink-4">
                        {countLine(row.count)}
                      </small>
                      {!row.goal && (
                        <small className="mt-1 block text-micro text-ink-4">
                          {t('settings.historical')}
                        </small>
                      )}
                    </div>
                  </div>
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-3 pr-6">
                    {row.state != null && row.budgetPercentage != null ? (
                      <>
                        <Progress
                          value={row.budgetPercentage}
                          over={row.state === 'over'}
                          className="h-[7px] flex-1"
                          aria-label={fmt.percent(row.budgetPercentage / 100, 0)}
                        />
                        <small className="w-9 text-right text-micro font-650 text-ink-4 num">
                          {fmt.percent(row.budgetPercentage / 100, 0)}
                        </small>
                      </>
                    ) : (
                      <span>—</span>
                    )}
                  </div>
                </TableCell>
                <TableCell className="text-right num">
                  <b className="block font-650 text-ink">
                    {row.budgetCurrency && row.budgetSpent != null
                      ? nativeMoney(Number(row.budgetSpent), row.budgetCurrency)
                      : '—'}
                  </b>
                  <small className="mt-0.5 block text-micro font-500 text-ink-4">
                    {row.budgetCurrency && row.budgetLimitNative != null
                      ? t('table.of', {
                          amount: nativeMoney(Number(row.budgetLimitNative), row.budgetCurrency),
                        })
                      : t('readiness.usageUnavailable')}
                  </small>
                  {row.goal &&
                    (() => {
                      const next = scheduledVersions(row.goal.versions, currentMonthStart())[0];
                      if (!next) return null;
                      const month = fmt.monthInSentence(new Date(next.effectiveMonth * 1000));
                      return (
                        <small className="mt-0.5 block text-micro font-500 text-ink-4">
                          {next.amount == null
                            ? t('readiness.scheduledRemoval', { month })
                            : t('readiness.scheduledLimit', {
                                month,
                                amount: nativeMoney(Number(next.amount), next.currency),
                              })}
                        </small>
                      );
                    })()}
                </TableCell>
                <TableCell className="text-right">{stateBadge(row)}</TableCell>
                <TableCell className="pr-3 text-right">
                  <div data-row-actions onClick={(e) => e.stopPropagation()}>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          aria-label={tc('labels.moreActions')}
                        >
                          <Ellipsis />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem
                          onSelect={() =>
                            row.goal ? openEdit(row) : openAdd(row.categoryId, range.start)
                          }
                        >
                          {t(row.goal ? 'actions.editLimit' : 'actions.setLimit')}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          disabled={row.count === 0}
                          onSelect={() => openTransactions(row)}
                        >
                          {t('actions.showTransactions')}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
          <span>{t('settings.evaluationNote', { period: period.label })}</span>
          <Link
            href="/reports/cashflow"
            className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
          >
            {t('table.cashflowLink')} →
          </Link>
        </div>
      </Card>

      {unbudgetedCard}
      {dialogs}
    </>
  );
}
