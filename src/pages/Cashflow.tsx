import { useMemo, useState, type ReactNode } from 'react';
import { Link } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Landmark } from 'lucide-react';
import type { CashflowActuals, CashflowGroup, CashflowRange } from '@shared/schema';
import { cashflowApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useShellPage } from '@/components/shell/shell-context';
import { categoryDisplayName } from '@/utils/category-name';
import { PageHead } from '@/components/shell/PageHead';
import { Segmented } from '@/components/ui/segmented';
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

const RANGES: readonly CashflowRange[] = ['6m', '12m', 'ytd'];

interface Row {
  key: string;
  label: string;
  amount: number;
  previous: number;
  share: number;
}

/**
 * Cashflow report (design system §7 Report, §11): actual monthly income and
 * expenses from bank transactions, averages, expenses by category and income
 * by source with the change against the preceding range.
 */
export default function Cashflow() {
  const { t } = useTranslation('reports');
  const { t: tBank } = useTranslation('bank_accounts');
  const { formatCurrency, formatCurrencySigned, convert, currencyCode } = useCurrency();
  const fmt = useFormat();
  const [range, setRange] = useState<CashflowRange>('12m');

  // Prefix shared with the planned report, so the mutation contract's
  // ['cashflow-report'] invalidation refreshes this page too.
  const { data, isLoading } = useQuery<CashflowActuals>({
    queryKey: ['cashflow-report', 'actuals', range],
    queryFn: () => cashflowApi.getActuals(range),
    staleTime: 0,
    refetchOnMount: 'always',
  });

  const czk = (v: string | number) => Number(v) || 0;
  const money = (v: number) => formatCurrency(v);
  const monthCount = data?.months.length ?? 0;
  const totalIncome = czk(data?.totalIncome ?? 0);
  const totalExpenses = czk(data?.totalExpenses ?? 0);
  const prevIncome = czk(data?.previousIncome ?? 0);
  const prevExpenses = czk(data?.previousExpenses ?? 0);
  const savingsRate = totalIncome > 0 ? (totalIncome - totalExpenses) / totalIncome : 0;
  const prevRate = prevIncome > 0 ? (prevIncome - prevExpenses) / prevIncome : null;

  const periodLabel =
    range === 'ytd'
      ? t('actuals.range.ytd', { year: new Date().getUTCFullYear() })
      : t(`actuals.range.${range}`);

  useShellPage({
    status: data
      ? {
          text: `${tBank('metrics.accounts', { count: data.accountCount })} · ${t('actuals.transactions', { count: data.transactionCount })} · ${periodLabel.toLocaleLowerCase(fmt.locale)}`,
          tone: 'neutral',
        }
      : undefined,
  });

  const groupLabel = (g: CashflowGroup, kind: 'expense' | 'income') => {
    if (kind === 'expense') {
      if (g.key === 'uncategorized') return t('actuals.uncategorized');
      return categoryDisplayName(
        { id: g.key, name: g.name, isSystem: g.key.startsWith('cat_') },
        tBank
      );
    }
    return g.name || t('actuals.otherIncome');
  };

  // Top N rows plus "Ostatní"; shares against the current total
  const fold = (groups: CashflowGroup[], kind: 'expense' | 'income', max: number): Row[] => {
    const total = groups.reduce((s, g) => s + Math.max(0, czk(g.amount)), 0);
    const rows = groups
      .map((g) => ({
        key: g.key,
        label: groupLabel(g, kind),
        amount: czk(g.amount),
        previous: czk(g.previousAmount),
        share: total > 0 ? Math.max(0, czk(g.amount)) / total : 0,
      }))
      .filter((r) => r.amount > 0 || r.previous > 0);
    if (rows.length <= max) return rows;
    const head = rows.slice(0, max - 1);
    const rest = rows.slice(max - 1);
    head.push({
      key: 'other',
      label: t('actuals.other'),
      amount: rest.reduce((s, r) => s + r.amount, 0),
      previous: rest.reduce((s, r) => s + r.previous, 0),
      share: rest.reduce((s, r) => s + r.share, 0),
    });
    return head;
  };

  const expenseRows = useMemo(
    () => (data ? fold(data.expenseCategories, 'expense', 7) : []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [data, fmt.locale]
  );
  const incomeRows = useMemo(
    () => (data ? fold(data.incomeSources, 'income', 5) : []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [data, fmt.locale]
  );

  const bars = useMemo(
    () =>
      (data?.months ?? []).map((m) => ({
        label: fmt.month(new Date(m.month * 1000), { month: 'short', year: '2-digit' }),
        a: czk(m.income),
        b: czk(m.expenses),
      })),
    [data, fmt]
  );

  const topSource = incomeRows[0];
  const topExpense = expenseRows[0];

  const exportCsv = async () => {
    if (!data) return { csv: '', filename: 'cashflow.csv', count: 0 };
    // Amounts are CZK; the file carries them in the main currency, like the page
    const suffix = currencyCode.toLowerCase();
    const inMain = (v: string | number) =>
      Math.round(convert(czk(v), 'CZK', currencyCode) * 100) / 100;
    const lines = [
      ['month', `income_${suffix}`, `expenses_${suffix}`].join(';'),
      ...data.months.map((m) =>
        [
          fmt.day(m.month, { year: 'numeric', month: '2-digit' }),
          inMain(m.income),
          inMain(m.expenses),
        ].join(';')
      ),
      '',
      ['expense_category', `amount_${suffix}`, `previous_${suffix}`, 'transactions'].join(';'),
      ...data.expenseCategories.map((g) =>
        [groupLabel(g, 'expense'), inMain(g.amount), inMain(g.previousAmount), g.count].join(';')
      ),
      '',
      ['income_source', `amount_${suffix}`, `previous_${suffix}`, 'transactions'].join(';'),
      ...data.incomeSources.map((g) =>
        [groupLabel(g, 'income'), inMain(g.amount), inMain(g.previousAmount), g.count].join(';')
      ),
    ];
    return {
      csv: lines.join('\n'),
      filename: `cashflow-${range}.csv`,
      count: data.months.length + data.expenseCategories.length + data.incomeSources.length,
    };
  };

  const changeCell = (row: Row, lowerIsBetter: boolean): ReactNode => {
    if (row.previous <= 0) {
      return <span className="text-ink-4">{row.amount > 0 ? t('actuals.new') : '—'}</span>;
    }
    const change = (row.amount - row.previous) / row.previous;
    if (Math.abs(change) < 0.0005)
      return <span className="text-ink-4">{t('actuals.noChange')}</span>;
    const good = lowerIsBetter ? change < 0 : change > 0;
    return (
      <span className={cn('font-650', good ? 'text-gain' : 'text-ink-2')}>
        {fmt.percent(change, 1, { signed: true })}
      </span>
    );
  };

  const breakdownTable = (
    rows: Row[],
    firstHead: string,
    lowerIsBetter: boolean,
    footer: ReactNode
  ) => (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead className="w-[30%]">{firstHead}</TableHead>
          <TableHead>{t('actuals.breakdown.share')}</TableHead>
          <TableHead className="text-right">{t('actuals.breakdown.amount')}</TableHead>
          <TableHead className="text-right">{t('actuals.breakdown.vsPrevious')}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((row, i) => (
          <TableRow key={row.key} className="h-[46px]">
            <TableCell className="font-650 text-ink">{row.label}</TableCell>
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
            <TableCell className="text-right num">{changeCell(row, lowerIsBetter)}</TableCell>
            {i === -1 && null}
          </TableRow>
        ))}
      </TableBody>
      {footer}
    </Table>
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
          <Segmented
            size="lg"
            value={range}
            onValueChange={setRange}
            options={RANGES.map((r) => ({
              value: r,
              label:
                r === 'ytd'
                  ? t('actuals.range.ytd', { year: new Date().getUTCFullYear() })
                  : t(`actuals.range.${r}`),
            }))}
          />
          <ExportButton exportFn={exportCsv} label={t('actuals.export')} />
        </>
      }
    />
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

  if (data.transactionCount === 0 && prevIncome === 0 && prevExpenses === 0) {
    return (
      <>
        {head}
        <EmptyState
          icon={<Landmark />}
          title={t('actuals.empty.title')}
          description={t('actuals.empty.description')}
          action={
            <Button asChild>
              <Link href="/bank-accounts">{t('actuals.empty.action')}</Link>
            </Button>
          }
        />
      </>
    );
  }

  const avgIncome = monthCount > 0 ? totalIncome / monthCount : 0;
  const avgExpenses = monthCount > 0 ? totalExpenses / monthCount : 0;
  const avgSavings = avgIncome - avgExpenses;

  return (
    <>
      {head}

      <Stats columns={3}>
        <Stat
          label={t('actuals.stats.avgIncome')}
          value={money(avgIncome)}
          note={
            topSource
              ? t('actuals.stats.avgIncomeNote', {
                  months: t('actuals.months', { count: monthCount }),
                  source: topSource.label,
                  percent: fmt.percent(topSource.share, 0),
                })
              : t('actuals.months', { count: monthCount })
          }
        />
        <Stat
          label={t('actuals.stats.avgExpenses')}
          value={money(avgExpenses)}
          note={
            topExpense
              ? t('actuals.stats.avgExpensesNote', {
                  category: topExpense.label,
                  amount: money(topExpense.amount / Math.max(1, monthCount)),
                })
              : t('actuals.months', { count: monthCount })
          }
        />
        <Stat
          label={t('actuals.stats.avgSavings')}
          value={formatCurrencySigned(avgSavings)}
          tone={avgSavings > 0 ? 'gain' : avgSavings < 0 ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={
            prevRate !== null
              ? t('actuals.stats.avgSavingsNote', {
                  rate: fmt.percent(savingsRate, 0),
                  previous: fmt.percent(prevRate, 0),
                })
              : t('actuals.stats.avgSavingsNoteNoPrev', { rate: fmt.percent(savingsRate, 0) })
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
              { label: t('actuals.chart.expenses'), swatch: { kind: 'block', color: 'var(--s3)' } },
            ]}
          />
        </div>
        <MoonyBarChart
          className="relative z-[1] -mx-1 mt-[18px]"
          bars={bars}
          names={[t('actuals.chart.income'), t('actuals.chart.expenses')]}
          height={200}
          formatValue={money}
          renderExtra={(bar) =>
            `${t('actuals.chart.balance')} ${formatCurrencySigned(bar.a - bar.b)}`
          }
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
          {breakdownTable(expenseRows, t('actuals.breakdown.category'), true, null)}
          <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
            <span className="num">
              {t('actuals.breakdown.total', { amount: money(totalExpenses) })}
            </span>
            <Link
              href="/reports/budgeting"
              className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
            >
              {t('actuals.expenses.budgets')} →
            </Link>
          </div>
        </Card>
        <Card variant="table">
          <CardHeader>
            <div>
              <CardTitle>{t('actuals.income.title')}</CardTitle>
              <CardDescription>{t('actuals.breakdown.sub')}</CardDescription>
            </div>
          </CardHeader>
          {breakdownTable(incomeRows, t('actuals.breakdown.source'), false, null)}
          <div className="flex items-center justify-between border-t border-line px-[14px] py-3 text-micro font-500 text-ink-4">
            <span className="num">
              {t('actuals.breakdown.total', { amount: money(totalIncome) })}
            </span>
            <span>{t('actuals.income.note')}</span>
          </div>
        </Card>
      </div>
    </>
  );
}
