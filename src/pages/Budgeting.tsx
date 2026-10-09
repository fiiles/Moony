import { AccountScope } from '@/components/reports/AccountScope';
import { ReportQualityNote, ReportError } from '@/components/reports/ReportQualityNote';
import { useReportMoney } from '@/lib/use-report-money';
import { useCurrency } from '@/lib/currency';
import { summarizeBudgetStatus } from '@/utils/report-currencies';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { translateApiError } from '@/lib/translate-api-error';
import { useMemo, useState } from 'react';
import { Link } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Ellipsis, Landmark, PiggyBank, Plus } from 'lucide-react';
import { budgetingApi, type BudgetGoal } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useShellPage } from '@/components/shell/shell-context';
import { usePeriodSelection } from '@/hooks/use-period-selection';
import { PeriodNav } from '@/components/reports/PeriodNav';
import type { BudgetState } from '@/utils/budget-pace';
import { currentMonthStart, scheduledVersions } from '@/utils/budget-goal-versions';
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
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { EmptyState } from '@/components/common/EmptyState';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { BudgetGoalDialog } from '@/components/budgeting/BudgetGoalDialog';
import {
  CategoryTransactionsDialog,
  type CategoryTransactionsTarget,
} from '@/components/budgeting/CategoryTransactionsDialog';

type SortKey = 'usage' | 'name';

interface BudgetRow {
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
  const { currencyCode, ratesTimestamp } = useCurrency();
  const [accountIds, setAccountIds] = useState<string[] | undefined>(undefined);
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const categoryName = useCategoryName();

  const period = usePeriodSelection('monthly');
  const { timeframe, range, label: periodLabel } = period;
  const [sort, setSort] = useState<SortKey>('usage');
  const [goalDialog, setGoalDialog] = useState<{
    open: boolean;
    goal: BudgetGoal | null;
    categoryId: string | null;
  }>({ open: false, goal: null, categoryId: null });
  const [pendingDelete, setPendingDelete] = useState<BudgetRow | null>(null);
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
      ratesTimestamp,
    ],
    queryFn: () =>
      budgetingApi.getReport(range.start, range.end, timeframe, accountIds, currencyCode),
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
  const { categories, isLoading: categoriesLoading } = useCategories();
  const { accounts } = useBankAccounts();

  const deleteMutation = useMutation({
    mutationFn: (id: string) => budgetingApi.deleteBudgetGoal(id),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast.success(t('deleteConfirm.deleted'));
      setPendingDelete(null);
    },
    onError: (error: Error) => toast.error(translateApiError(error, tc)),
  });

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
    const ids = new Set([
      ...(report?.expenseCategories ?? []).filter((c) => c.budgetGoal).map((c) => c.categoryId),
      ...goals.filter((g) => g.timeframe === 'monthly').map((g) => g.categoryId),
    ]);
    const list = [...ids].map((categoryId) => {
      const goal =
        goals.find((g) => g.categoryId === categoryId && g.timeframe === 'monthly') ?? null;
      const historical = report?.expenseCategories.find((c) => c.categoryId === categoryId);
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

  const availableCategories = useMemo(() => {
    const budgeted = new Set(goals.map((g) => g.categoryId));
    return categories.filter((c) => c.kind === 'expense' && !budgeted.has(c.id));
  }, [categories, goals]);

  const status = summarizeBudgetStatus(rows, report?.totalExpenses ?? '0');

  // Czech month names are lower-case inside a sentence; English keeps the capital.
  const inSentence = (text: string) =>
    fmt.locale.startsWith('cs') ? text.toLocaleLowerCase(fmt.locale) : text;
  // "Q4 2026" and "2026" keep their spelling everywhere
  const periodInSentence = timeframe === 'monthly' ? inSentence(periodLabel) : periodLabel;

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

  const openAdd = (categoryId: string | null = null) =>
    setGoalDialog({ open: true, goal: null, categoryId });
  const openEdit = (row: BudgetRow) =>
    setGoalDialog({ open: true, goal: row.goal, categoryId: row.categoryId });

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

  const head = (
    <PageHead
      eyebrow={t('eyebrow')}
      title={t('title')}
      description={t('subtitle')}
      actions={
        <>
          <PeriodNav selection={period} />
          <AccountScope value={accountIds} onChange={setAccountIds} />
          <Button onClick={() => openAdd()} disabled={availableCategories.length === 0}>
            <Plus />
            {t('actions.add')}
          </Button>
        </>
      }
    />
  );

  const dialogs = (
    <>
      <BudgetGoalDialog
        open={goalDialog.open}
        onOpenChange={(open) => setGoalDialog((d) => ({ ...d, open }))}
        goal={goalDialog.goal}
        initialCategoryId={goalDialog.categoryId}
        availableCategories={availableCategories}
        categories={categories}
      />
      <CategoryTransactionsDialog
        accountIds={accountIds}
        target={txTarget}
        reportCurrency={txTarget?.budgetCurrency ?? report?.reportCurrency ?? currencyCode}
        ratesTimestamp={ratesTimestamp}
        onClose={() => setTxTarget(null)}
        start={range.start}
        end={range.end}
        periodLabel={periodInSentence}
      />
      <ConfirmDeleteDialog
        open={!!pendingDelete}
        onOpenChange={(open) => !open && setPendingDelete(null)}
        title={t('deleteConfirm.title')}
        description={t('deleteConfirm.description', { category: pendingDelete?.name ?? '' })}
        confirmLabel={t('actions.delete')}
        isPending={deleteMutation.isPending}
        onConfirm={() => pendingDelete && deleteMutation.mutate(pendingDelete.goal!.id)}
      />
    </>
  );

  if (reportError || goalsError)
    return (
      <>
        {head}
        <ReportError
          onRetry={() => {
            void refetchReport();
            void refetchGoals();
          }}
        />
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
            <TableRow key={row.categoryId} className="h-[52px]">
              <TableCell>
                <button
                  type="button"
                  className="block text-left font-650 text-ink underline-offset-[3px] hover:underline focus-visible:outline-none focus-visible:underline"
                  onClick={() => setTxTarget({ categoryId: row.categoryId, name: row.name })}
                >
                  {row.name}
                </button>
                <small className="mt-[3px] block text-micro font-500 text-ink-4">
                  {countLine(row.count)}
                </small>
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
                  <Button variant="ghost" size="sm" onClick={() => openAdd(row.categoryId)}>
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
          title={t('noBudgets.title')}
          description={t('noBudgets.description')}
          action={
            <Button onClick={() => openAdd()}>
              <Plus />
              {t('actions.add')}
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
              <TableRow key={row.categoryId} className="h-[60px]">
                <TableCell>
                  <button
                    type="button"
                    className="block text-left font-650 text-ink underline-offset-[3px] hover:underline focus-visible:outline-none focus-visible:underline"
                    onClick={() =>
                      setTxTarget({
                        categoryId: row.categoryId,
                        name: row.name,
                        budgetCurrency: row.budgetCurrency ?? row.goal?.currency ?? undefined,
                      })
                    }
                  >
                    {row.name}
                  </button>
                  <small className="mt-[3px] block text-micro font-500 text-ink-4">
                    {countLine(row.count)}
                  </small>
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
                      const month = fmt.month(new Date(next.effectiveMonth * 1000));
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
                  <div data-row-actions>
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
                        <DropdownMenuItem onSelect={() => openEdit(row)}>
                          {t('actions.editLimit')}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          disabled={row.count === 0}
                          onSelect={() =>
                            setTxTarget({
                              categoryId: row.categoryId,
                              name: row.name,
                              budgetCurrency: row.budgetCurrency ?? row.goal?.currency ?? undefined,
                            })
                          }
                        >
                          {t('actions.showTransactions')}
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        <DropdownMenuItem
                          disabled={!row.goal}
                          variant="danger"
                          onSelect={() => setPendingDelete(row)}
                        >
                          {t('actions.delete')}
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
          <span>{t('table.footer', { count: rows.length })}</span>
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
