import { AccountScope } from '@/components/reports/AccountScope';
import { PeriodNav } from '@/components/reports/PeriodNav';
import { ReportQualityNote, ReportError } from '@/components/reports/ReportQualityNote';
import {
  ReportTransactionsDialog,
  type ReportTarget,
} from '@/components/reports/ReportTransactionsDialog';
import { useReportMoney } from '@/lib/use-report-money';
import { usePeriodSelection } from '@/hooks/use-period-selection';
import { buildCashflowCsv } from '@/utils/report-currencies';
import { absoluteMovementShares } from '@/utils/money-flows';
import { periodFileStem } from '@/utils/period';
import { useMemo, useState, type ReactNode } from 'react';
import { Link } from 'wouter';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Landmark } from 'lucide-react';
import type { CashflowActuals, CashflowGroup } from '@shared/schema';
import { cashflowApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useShellPage } from '@/components/shell/shell-context';
import { categoryDisplayName } from '@/utils/category-name';
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
import { ExportButton } from '@/components/common/ExportButton';
import { MoonyBarChart } from '@/components/charts/MoonyBarChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { cn } from '@/lib/utils';

interface Row {
  group: CashflowGroup;
  label: string;
  amount: number;
  share: number;
}

/**
 * Cashflow report (design system §7 Report, §11): actual income and expenses
 * from bank transactions for a calendar month, quarter or year, by category,
 * with money transfers (investing, saving, between accounts) apart. Every
 * breakdown row opens its transactions, where they can be recategorized.
 */
export default function Cashflow() {
  const { t } = useTranslation('reports');
  const { t: tBank } = useTranslation('bank_accounts');
  const { currencyCode, ratesTimestamp } = useCurrency();
  const [accountIds, setAccountIds] = useState<string[] | undefined>(undefined);
  const [txTarget, setTxTarget] = useState<ReportTarget | null>(null);
  const fmt = useFormat();
  const period = usePeriodSelection('yearly');
  const { range, timeframe } = period;

  // Prefix shared with the planned report, so the mutation contract's
  // ['cashflow-report'] invalidation refreshes this page too.
  const { data, isLoading, isError, isPlaceholderData, refetch } = useQuery<CashflowActuals>({
    queryKey: [
      'cashflow-report',
      'actuals',
      range.start,
      range.end,
      accountIds,
      currencyCode,
      ratesTimestamp,
    ],
    queryFn: () => cashflowApi.getActuals(range.start, range.end, accountIds, currencyCode),
    staleTime: 0,
    refetchOnMount: 'always',
    placeholderData: keepPreviousData,
  });
  const { money, signedMoney } = useReportMoney(data?.reportCurrency);

  // Czech month names are lower-case inside a sentence; "Q4 2026" and "2026" keep their spelling.
  const periodInSentence =
    timeframe === 'monthly' && fmt.locale.startsWith('cs')
      ? period.label.toLocaleLowerCase(fmt.locale)
      : period.label;

  useShellPage({
    status: data
      ? {
          text: `${tBank('metrics.accounts', { count: data.accountCount })} · ${t('actuals.transactions', { count: data.transactionCount })} · ${periodInSentence}`,
          tone: 'neutral',
        }
      : undefined,
  });

  const amount = (v: string | number) => Number(v) || 0;

  const groupLabel = (g: CashflowGroup) => {
    if (g.key === 'uncategorized') {
      return g.flowKind === 'transfer'
        ? t('actuals.uncategorizedTransfers')
        : t('actuals.uncategorized');
    }
    return categoryDisplayName(
      { id: g.key, name: g.name, isSystem: g.key.startsWith('cat_') },
      tBank
    );
  };

  // Shares use absolute movement, amounts keep their signs.
  const toRows = (groups: CashflowGroup[]): Row[] => {
    const shares = absoluteMovementShares(groups.map((g) => amount(g.amount)));
    return groups
      .map((group, index) => ({
        group,
        label: groupLabel(group),
        amount: amount(group.amount),
        share: shares[index],
      }))
      .filter((row) => row.group.count > 0 || row.amount !== 0);
  };

  const expenseRows = useMemo(
    () => (data ? toRows(data.expenseCategories) : []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [data, fmt.locale]
  );
  const incomeRows = useMemo(
    () => (data ? toRows(data.incomeCategories) : []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [data, fmt.locale]
  );
  const transferRows = useMemo(
    () => (data ? toRows(data.transfers) : []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [data, fmt.locale]
  );

  const bars = useMemo(
    () =>
      (data?.months ?? []).map((m) => ({
        label: fmt.month(new Date(m.month * 1000), { month: 'short', year: '2-digit' }),
        a: amount(m.income),
        b: amount(m.expenses),
        muted: m.month < data!.periodStart || m.month > data!.periodEnd,
      })),
    [data, fmt]
  );

  const exportCsv = async () => {
    if (!data || isPlaceholderData) return { csv: '', filename: 'cashflow.csv', count: 0 };
    return {
      csv: buildCashflowCsv(
        data,
        (month) => fmt.day(month, { year: 'numeric', month: '2-digit' }),
        groupLabel
      ),
      filename: `cashflow-${periodFileStem(timeframe, range.startDate)}.csv`,
      count:
        data.months.length +
        data.expenseCategories.length +
        data.incomeCategories.length +
        data.transfers.length,
    };
  };

  const open = (row: Row) =>
    setTxTarget({
      name: row.label,
      flowKind: row.group.flowKind,
      categoryId: row.group.key,
      expectedCount: row.group.count,
    });

  const breakdownTable = (rows: Row[]) => (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead className="w-[42%]">{t('actuals.breakdown.category')}</TableHead>
          <TableHead>{t('actuals.breakdown.share')}</TableHead>
          <TableHead className="text-right">{t('actuals.breakdown.amount')}</TableHead>
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
            className="h-[46px] cursor-pointer"
            onClick={() => open(row)}
          >
            <TableCell className="font-650 text-ink">
              <button
                type="button"
                className="text-left focus-visible:underline"
                onClick={(event) => {
                  event.stopPropagation();
                  open(row);
                }}
              >
                {row.label} <small className="text-ink-4">· {row.group.count}</small>
              </button>
            </TableCell>
            <TableCell>
              <div className="flex items-center gap-[10px]">
                <Progress
                  value={row.share * 100}
                  className="h-[5px] flex-1"
                  aria-label={fmt.percent(row.share, 0)}
                />
                <small className="w-9 text-right text-micro font-600 text-ink-4 num">
                  {fmt.percent(row.share, 0)}
                </small>
              </div>
            </TableCell>
            <TableCell className="text-right font-650 text-ink num">{money(row.amount)}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );

  const footer = (left: ReactNode, right?: ReactNode) => (
    <div className="flex items-center justify-between gap-4 border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
      <span className="num">{left}</span>
      {right}
    </div>
  );

  const head = (
    <PageHead
      eyebrow={t('actuals.eyebrow')}
      title={t('actuals.title')}
      description={
        <>
          {t('actuals.subtitle')}{' '}
          <Link
            href="/reports/cashflow-planning"
            className="font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
          >
            {t('actuals.link')} →
          </Link>
        </>
      }
      actions={
        <>
          <PeriodNav selection={period} />
          <AccountScope value={accountIds} onChange={setAccountIds} />
          <ExportButton exportFn={exportCsv} label={t('actuals.export')} />
        </>
      }
    />
  );

  if (isError)
    return (
      <>
        {head}
        <ReportError onRetry={() => void refetch()} />
      </>
    );
  if (isLoading || !data) {
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

  if (data.transactionCount === 0) {
    return (
      <>
        {head}
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
      </>
    );
  }

  const totalIncome = amount(data.totalIncome);
  const totalExpenses = amount(data.totalExpenses);
  const net = amount(data.netBalance);
  const perMonth = (average: string | null) =>
    average != null && data.averageCompletedMonths.length >= 2
      ? t('actuals.stats.perMonth', { amount: money(amount(average)) })
      : undefined;

  return (
    <>
      {head}
      <div
        className={cn('transition-opacity', isPlaceholderData && 'opacity-60')}
        aria-busy={isPlaceholderData}
      >
        <ReportQualityNote quality={data.quality} />
        <ReportTransactionsDialog
          target={txTarget}
          start={data.periodStart}
          end={data.periodEnd}
          accountIds={accountIds}
          reportCurrency={data.reportCurrency}
          ratesTimestamp={ratesTimestamp}
          onClose={() => setTxTarget(null)}
        />
        <Stats columns={3}>
          <Stat
            label={t('actuals.stats.income')}
            value={money(totalIncome)}
            note={perMonth(data.averageIncome)}
          />
          <Stat
            label={t('actuals.stats.expenses')}
            value={money(totalExpenses)}
            note={perMonth(data.averageExpenses)}
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
          />
        </Card>

        <div className="grid grid-cols-2 items-start gap-[18px]">
          <Card variant="table">
            <CardHeader>
              <div>
                <CardTitle>{t('actuals.expenses.title')}</CardTitle>
                <CardDescription>{t('actuals.breakdown.sub')}</CardDescription>
              </div>
            </CardHeader>
            {breakdownTable(expenseRows)}
            {footer(
              t('actuals.breakdown.total', { amount: money(totalExpenses) }),
              <Link
                href="/reports/budgeting"
                className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
              >
                {t('actuals.expenses.budgets')} →
              </Link>
            )}
          </Card>
          <div className="flex flex-col gap-[18px]">
            <Card variant="table">
              <CardHeader>
                <div>
                  <CardTitle>{t('actuals.income.title')}</CardTitle>
                  <CardDescription>{t('actuals.breakdown.sub')}</CardDescription>
                </div>
              </CardHeader>
              {breakdownTable(incomeRows)}
              {footer(
                t('actuals.breakdown.total', { amount: money(totalIncome) }),
                <span>{t('actuals.income.note')}</span>
              )}
            </Card>
            {transferRows.length > 0 && (
              <Card variant="table">
                <CardHeader>
                  <div>
                    <CardTitle>{t('actuals.transfers.title')}</CardTitle>
                    <CardDescription>{t('actuals.transfers.sub')}</CardDescription>
                  </div>
                </CardHeader>
                {breakdownTable(transferRows)}
                {footer(
                  t('actuals.transfers.movement', {
                    amount: signedMoney(amount(data.bankCashMovement)),
                  })
                )}
              </Card>
            )}
          </div>
        </div>
      </div>
    </>
  );
}
