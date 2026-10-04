import { useMemo, useState } from 'react';
import { Link } from 'wouter';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { ChevronLeft, ChevronRight, Ellipsis, Landmark, PiggyBank, Plus } from 'lucide-react';
import type { CurrencyCode } from '@shared/currencies';
import { budgetingApi, type BudgetGoal } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useShellPage } from '@/components/shell/shell-context';
import { getUtcPeriodRange, type PeriodTimeframe } from '@/utils/period';
import {
  budgetMultiplier,
  budgetState,
  paceVerdict,
  periodPace,
  type BudgetState,
} from '@/utils/budget-pace';
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
import { cn } from '@/lib/utils';

const TIMEFRAMES: readonly PeriodTimeframe[] = ['monthly', 'quarterly', 'yearly'];
const EXCLUDED = new Set(['cat_income', 'cat_internal_transfers']);

type SortKey = 'usage' | 'name';

interface BudgetRow {
  goal: BudgetGoal;
  categoryId: string;
  name: string;
  /** Limit scaled to the period, CZK. */
  limit: number;
  /** Spent in the period, CZK. */
  spent: number;
  count: number;
  ratio: number;
  state: BudgetState;
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
 * navigation, three stats with the pace of the period, the overall usage bar
 * with today's mark, one table of limited categories with state badges, and
 * the spending that has no limit yet.
 */
export default function Budgeting() {
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  const { formatCurrency, convert } = useCurrency();
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const categoryName = useCategoryName();

  const [timeframe, setTimeframe] = useState<PeriodTimeframe>('monthly');
  const [periodOffset, setPeriodOffset] = useState(0);
  const [sort, setSort] = useState<SortKey>('usage');
  const [goalDialog, setGoalDialog] = useState<{
    open: boolean;
    goal: BudgetGoal | null;
    categoryId: string | null;
  }>({ open: false, goal: null, categoryId: null });
  const [pendingDelete, setPendingDelete] = useState<BudgetRow | null>(null);
  const [txTarget, setTxTarget] = useState<CategoryTransactionsTarget | null>(null);

  // Period boundaries on the UTC calendar: booking dates are UTC midnight
  // (ADR 0008), so local boundaries would drop the last day east of UTC.
  const range = useMemo(
    () => getUtcPeriodRange(timeframe, periodOffset),
    [timeframe, periodOffset]
  );
  const periodLabel = useMemo(() => {
    if (timeframe === 'monthly') return fmt.month(range.startDate);
    if (timeframe === 'quarterly') {
      return `Q${Math.floor(range.startDate.getUTCMonth() / 3) + 1} ${range.startDate.getUTCFullYear()}`;
    }
    return String(range.startDate.getUTCFullYear());
  }, [timeframe, range, fmt]);
  const pace = useMemo(() => periodPace(range.start, range.end), [range]);
  const multiplier = budgetMultiplier(timeframe);

  const { data: report, isLoading: reportLoading } = useQuery({
    queryKey: ['budgeting-report', range.start, range.end, timeframe],
    queryFn: () => budgetingApi.getReport(range.start, range.end, timeframe),
  });
  const { data: goals = [], isLoading: goalsLoading } = useQuery({
    queryKey: ['budget-goals'],
    queryFn: () => budgetingApi.getBudgetGoals(),
  });
  const { categories, isLoading: categoriesLoading } = useCategories();
  const { accounts } = useBankAccounts();

  const deleteMutation = useMutation({
    mutationFn: (id: string) => budgetingApi.deleteBudgetGoal(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['budgeting-report'] });
      queryClient.invalidateQueries({ queryKey: ['budget-goals'] });
      toast.success(t('deleteConfirm.deleted'));
      setPendingDelete(null);
    },
    onError: (error: Error) => toast.error(error.message),
  });

  // Spent per category (CZK, positive) for the period
  const spentByCategory = useMemo(() => {
    const map = new Map<string, { spent: number; count: number }>();
    for (const c of report?.expenseCategories ?? []) {
      if (EXCLUDED.has(c.categoryId)) continue;
      map.set(c.categoryId, {
        spent: Math.max(0, parseFloat(c.totalAmount) || 0),
        count: c.transactionCount,
      });
    }
    return map;
  }, [report]);

  const rows = useMemo<BudgetRow[]>(() => {
    const list = goals
      .filter((g) => g.timeframe === 'monthly' && !EXCLUDED.has(g.categoryId))
      .map((goal) => {
        const category = categories.find((c) => c.id === goal.categoryId);
        // Limits carry their own currency: compare in CZK
        const limit =
          convert(parseFloat(goal.amount) || 0, goal.currency as CurrencyCode, 'CZK') * multiplier;
        const usage = spentByCategory.get(goal.categoryId);
        const spent = usage?.spent ?? 0;
        return {
          goal,
          categoryId: goal.categoryId,
          name: category ? categoryName(category) : goal.categoryId,
          limit,
          spent,
          count: usage?.count ?? 0,
          ratio: limit > 0 ? spent / limit : 0,
          state: budgetState(spent, limit),
        };
      });
    return list.sort((a, b) =>
      sort === 'name'
        ? a.name.localeCompare(b.name, fmt.locale)
        : b.ratio - a.ratio || b.spent - a.spent
    );
  }, [goals, categories, categoryName, convert, multiplier, spentByCategory, sort, fmt.locale]);

  const totalExpenses =
    Array.from(spentByCategory.values()).reduce((s, v) => s + v.spent, 0) +
    Math.max(0, parseFloat(report?.uncategorizedExpenses ?? '0') || 0);

  const unbudgeted = useMemo<UnbudgetedRow[]>(() => {
    const budgeted = new Set(rows.map((r) => r.categoryId));
    const list: UnbudgetedRow[] = [];
    for (const [categoryId, usage] of spentByCategory) {
      if (budgeted.has(categoryId) || usage.spent <= 0) continue;
      const category = categories.find((c) => c.id === categoryId);
      list.push({
        categoryId,
        name: category ? categoryName(category) : categoryId,
        spent: usage.spent,
        count: usage.count,
        share: totalExpenses > 0 ? usage.spent / totalExpenses : 0,
        canBudget: true,
      });
    }
    const uncategorized = Math.max(0, parseFloat(report?.uncategorizedExpenses ?? '0') || 0);
    if (uncategorized > 0) {
      list.push({
        categoryId: 'uncategorized',
        name: t('unbudgeted.uncategorized'),
        spent: uncategorized,
        count: report?.uncategorizedTransactionCount ?? 0,
        share: totalExpenses > 0 ? uncategorized / totalExpenses : 0,
        canBudget: false,
      });
    }
    return list.sort((a, b) => b.spent - a.spent);
  }, [rows, spentByCategory, categories, categoryName, report, totalExpenses, t]);

  const availableCategories = useMemo(() => {
    const budgeted = new Set(goals.map((g) => g.categoryId));
    return categories.filter((c) => !EXCLUDED.has(c.id) && !budgeted.has(c.id));
  }, [categories, goals]);

  const budget = rows.reduce((s, r) => s + r.limit, 0);
  const spent = rows.reduce((s, r) => s + r.spent, 0);
  const remaining = budget - spent;
  const share = budget > 0 ? spent / budget : 0;
  const overCount = rows.filter((r) => r.state === 'over').length;
  const nearCount = rows.filter((r) => r.state === 'near').length;

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
    setGoalDialog({ open: true, goal: row.goal, categoryId: null });

  const money = (czk: number) => formatCurrency(czk);
  const periodWord = (key: 'budgetFor' | 'elapsed') =>
    t(`stats.${key}.${timeframe}`, {
      period:
        timeframe === 'monthly'
          ? inSentence(fmt.month(range.startDate, { month: 'long', year: undefined }))
          : periodLabel,
      percent: fmt.percent(pace.elapsed, 0),
    });

  const spentNote = pace.isCurrent
    ? `${t('stats.ofBudget', { percent: fmt.percent(share, 0) })} · ${periodWord('elapsed')}`
    : t('stats.ofBudget', { percent: fmt.percent(share, 0) });

  const remainingNote = (() => {
    if (remaining < 0) return t('stats.overNote', { percent: fmt.percent(share - 1, 0) });
    if (pace.isPast) return t('stats.unspent');
    if (!pace.isCurrent) return t('stats.ahead');
    if (pace.remainingDays === 0) return t('stats.lastDay');
    return t('stats.remainingNote', {
      days: t('stats.days', { count: pace.remainingDays }),
      perDay: money(remaining / pace.remainingDays),
    });
  })();

  const verdictText = (() => {
    if (pace.isPast) return t('overview.closed');
    if (!pace.isCurrent) return t('overview.notStarted');
    const { verdict, points } = paceVerdict(share, pace.elapsed);
    if (verdict === 'onPace') return t('overview.onPace');
    return t(`overview.${verdict}`, { count: points });
  })();

  const stateBadge = (row: BudgetRow) => {
    if (row.state === 'over') {
      return (
        <Badge variant="loss">
          {t('table.state.over', { amount: money(row.spent - row.limit) })}
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
          <Segmented
            value={timeframe}
            onValueChange={(next) => {
              setTimeframe(next);
              setPeriodOffset(0);
            }}
            options={TIMEFRAMES.map((tf) => ({ value: tf, label: t(`timeframe.${tf}`) }))}
            aria-label={tc('periods.label')}
          />
          <div className="mx-1 inline-flex items-center gap-1">
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t(`actions.prev.${timeframe}`)}
              onClick={() => setPeriodOffset((o) => o - 1)}
            >
              <ChevronLeft />
            </Button>
            <b className="min-w-[110px] text-center text-body font-650 text-ink">{periodLabel}</b>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t(`actions.next.${timeframe}`)}
              disabled={periodOffset >= 0}
              onClick={() => setPeriodOffset((o) => o + 1)}
            >
              <ChevronRight />
            </Button>
          </div>
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
        target={txTarget}
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
        onConfirm={() => pendingDelete && deleteMutation.mutate(pendingDelete.goal.id)}
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
    report.uncategorizedTransactionCount === 0;

  // nothing booked in the period and no limits to show
  if (rows.length === 0 && noTransactions) {
    return (
      <>
        {head}
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
            <TableHead>{t('unbudgeted.share')}</TableHead>
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
          {t('unbudgeted.footer', {
            amount: money(unbudgetedTotal),
            percent: fmt.percent(totalExpenses > 0 ? unbudgetedTotal / totalExpenses : 0, 0),
          })}
        </span>
      </div>
    </Card>
  );

  if (rows.length === 0) {
    return (
      <>
        {head}
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

  const todayLeft = Math.max(0, Math.min(100, pace.elapsed * 100));

  return (
    <>
      {head}

      <Stats columns={3}>
        <Stat
          label={periodWord('budgetFor')}
          value={money(budget)}
          note={t('stats.limited', { count: rows.length })}
        />
        <Stat label={t('stats.spent')} value={money(spent)} note={spentNote} />
        <Stat
          label={remaining < 0 ? t('stats.over') : t('stats.remaining')}
          value={money(Math.abs(remaining))}
          tone={remaining < 0 ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={remainingNote}
        />
      </Stats>

      <Card variant="flat" className="mb-[17px] px-[21px] py-5">
        <div className="grid grid-cols-[1fr_auto] items-center gap-6">
          <div>
            <div className="mb-2 flex items-baseline justify-between gap-4">
              <h3 className="m-0 text-h3 text-ink">{t('overview.title')}</h3>
              <span className="text-caption text-ink-3">{verdictText}</span>
            </div>
            <Progress
              value={share * 100}
              over={share > 1}
              mark={pace.isCurrent ? todayLeft : undefined}
              className="h-2.5"
              aria-label={fmt.percent(share, 0)}
            />
            <div className="relative mt-1.5 flex justify-between text-[10px] font-600 text-ink-5">
              <span className={cn(pace.isCurrent && todayLeft < 12 && 'invisible')}>
                {fmt.day(range.start, { day: 'numeric', month: 'numeric' })}
              </span>
              {pace.isCurrent && (
                <span
                  className="absolute -translate-x-1/2 text-ink-3"
                  style={{ left: `${todayLeft}%` }}
                >
                  {tc('charts.today')}
                </span>
              )}
              <span className={cn(pace.isCurrent && todayLeft > 88 && 'invisible')}>
                {fmt.day(range.end, { day: 'numeric', month: 'numeric' })}
              </span>
            </div>
          </div>
          <div className="text-right">
            {overCount > 0 ? (
              <Badge variant="loss">{t('overview.overCount', { count: overCount })}</Badge>
            ) : nearCount > 0 ? (
              <Badge>{t('overview.nearCount', { count: nearCount })}</Badge>
            ) : (
              <Badge variant="gain">{t('overview.allOk')}</Badge>
            )}
          </div>
        </div>
      </Card>

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
                    onClick={() => setTxTarget({ categoryId: row.categoryId, name: row.name })}
                  >
                    {row.name}
                  </button>
                  <small className="mt-[3px] block text-micro font-500 text-ink-4">
                    {countLine(row.count)}
                  </small>
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-3 pr-6">
                    <Progress
                      value={row.ratio * 100}
                      over={row.state === 'over'}
                      className="h-[7px] flex-1"
                      aria-label={fmt.percent(row.ratio, 0)}
                    />
                    <small className="w-9 text-right text-micro font-650 text-ink-4 num">
                      {fmt.percent(row.ratio, 0)}
                    </small>
                  </div>
                </TableCell>
                <TableCell className="text-right num">
                  <b className="block font-650 text-ink">{money(row.spent)}</b>
                  <small className="mt-0.5 block text-micro font-500 text-ink-4">
                    {t('table.of', { amount: money(row.limit) })}
                  </small>
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
                            setTxTarget({ categoryId: row.categoryId, name: row.name })
                          }
                        >
                          {t('actions.showTransactions')}
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        <DropdownMenuItem variant="danger" onSelect={() => setPendingDelete(row)}>
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
