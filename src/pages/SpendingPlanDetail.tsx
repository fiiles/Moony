import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useRoute } from 'wouter';
import { ArrowUpRight, Lock, Pencil, RotateCcw, Trash2 } from 'lucide-react';
import type { SpendingPlanDetail as Detail } from '@shared/schema';
import {
  UNCATEGORIZED_KEY,
  categoryKey,
  cumulativeSpend,
  monthDate,
  monthOfDay,
  planCategoryRows,
  planMonthAxis,
  planMonthSpend,
  planStatus,
} from '@/utils/spending-plans';
import { useSpendingPlanDetail, useSpendingPlanTransactions } from '@/hooks/use-spending-plans';
import { useSpendingPlanMutations } from '@/hooks/use-spending-plan-mutations';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { useShellPage } from '@/components/shell/shell-context';
import { BackLink, PageHead } from '@/components/shell/PageHead';
import { Alert, AlertActions, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardTitle } from '@/components/ui/card';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { FilterBadge } from '@/components/common/FilterBadge';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { ConversionQualityNote, ReportError } from '@/components/reports/ReportQualityNote';
import { PlanChip } from '@/components/spending-plans/PlanChip';
import { PlanMoney } from '@/components/spending-plans/PlanMoney';
import { PlanFormDialog } from '@/components/spending-plans/PlanFormDialog';
import { PlanSpendChart } from '@/components/spending-plans/PlanSpendChart';
import { PlanCategoriesCard } from '@/components/spending-plans/PlanCategoriesCard';
import { PlanPaymentsCard } from '@/components/spending-plans/PlanPaymentsCard';
import { AddPaymentsDialog } from '@/components/spending-plans/AddPaymentsDialog';

type Modal = 'edit' | 'delete' | 'add' | null;

/**
 * A spending plan's detail (spec §9 "Detail page"): planned, spent and left; the spend per month
 * ("Útrata v čase"); the spend per category ("Kategorie") and the payments ("Platby"). A month
 * (chart) and a category (Kategorie) narrow the other widgets, each marked with a filter badge.
 */
export default function SpendingPlanDetail() {
  const { t } = useTranslation('spending_plans');
  const [, params] = useRoute('/spending-plans/:id');
  const [, setLocation] = useLocation();
  const detail = useSpendingPlanDetail(params?.id);
  const plan = detail.data?.summary.plan;

  useShellPage({ crumb: plan?.name });

  return (
    <>
      <BackLink href={plan ? `/spending-plans?year=${plan.year}` : '/spending-plans'}>
        {t('detail.back', { year: plan?.year ?? new Date().getFullYear() })}
      </BackLink>
      {detail.data ? (
        // A new plan starts without the previous one's filters and selection
        <PlanDetailBody
          key={detail.data.summary.plan.id}
          detail={detail.data}
          onDeleted={(year) => setLocation(`/spending-plans?year=${year}`)}
        />
      ) : detail.isError ? (
        <p className="mt-8 text-center text-ink-3">{t('detail.notFound')}</p>
      ) : (
        <>
          <div className="mb-[26px]">
            <div className="skeleton h-2.5 w-32" />
            <div className="skeleton mt-3 h-9 w-72" />
          </div>
          <Stats columns={3}>
            <StatSkeleton />
            <StatSkeleton />
            <StatSkeleton />
          </Stats>
        </>
      )}
    </>
  );
}

function PlanDetailBody({
  detail,
  onDeleted,
}: {
  detail: Detail;
  onDeleted: (year: number) => void;
}) {
  const { t } = useTranslation('spending_plans');
  const fmt = useFormat();
  const { summary, accountCount } = detail;
  const { plan, paymentCount, fxQuality } = summary;
  const transactions = useSpendingPlanTransactions(plan.id);
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const { accounts } = useBankAccounts();
  const { setClosed, deletePlan } = useSpendingPlanMutations();
  const [modal, setModal] = useState<Modal>(null);
  const [month, setMonth] = useState<string | null>(null);
  const [category, setCategory] = useState<string | null>(null);

  const closed = plan.closedAt !== null;
  const currency = plan.currency;
  const planned = parseFloat(plan.amount) || 0;
  const spent = summary.spent === null ? null : parseFloat(summary.spent);
  const status = planStatus(spent, planned, paymentCount);
  const ratio = spent === null || planned <= 0 ? null : spent / planned;
  const left = spent === null ? null : planned - spent;
  const over = left !== null && left < 0;
  const currentMonth = monthOfDay(Date.now() / 1000);

  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);
  const accountNames = useMemo(() => new Map(accounts.map((a) => [a.id, a.name])), [accounts]);

  const axis = useMemo(
    () =>
      planMonthAxis(
        plan.year,
        detail.months.map((m) => m.month)
      ),
    [plan.year, detail.months]
  );
  const bars = useMemo(() => planMonthSpend(detail, axis, category), [detail, axis, category]);
  // The running total and the plan line belong to the whole plan, not to one category
  const cumulative = useMemo(
    () =>
      category === null && paymentCount > 0 ? cumulativeSpend(bars, axis, currentMonth) : undefined,
    [category, paymentCount, bars, axis, currentMonth]
  );
  const categoryRows = useMemo(() => planCategoryRows(detail, month), [detail, month]);

  const all = useMemo(() => transactions.data ?? [], [transactions.data]);
  const shown = useMemo(
    () =>
      all.filter(
        (tx) =>
          (month === null || monthOfDay(tx.bookingDate) === month) &&
          (category === null || categoryKey(tx.categoryId) === category)
      ),
    [all, month, category]
  );

  // "červenec"; a month outside the plan year carries its year ("prosinec 2025")
  const monthLabel =
    month === null
      ? null
      : fmt.monthInSentence(monthDate(month), {
          month: 'long',
          ...(Number(month.slice(0, 4)) !== plan.year ? { year: 'numeric' } : {}),
        });
  const categoryLabel = (() => {
    if (category === null) return null;
    if (category === UNCATEGORIZED_KEY) return t('detail.categories.uncategorized');
    const found = categoryById.get(category);
    return found ? categoryName(found) : '—';
  })();
  const toggleMonth = (next: string) => setMonth((m) => (m === next ? null : next));
  const toggleCategory = (next: string) => setCategory((c) => (c === next ? null : next));

  const money = (value: number) => fmt.money(value, currency);
  const percent = ratio === null ? null : fmt.percent(ratio, 0);
  const spentAside =
    percent === null ? null : status === 'over' ? (
      <Badge variant="loss">
        <ArrowUpRight />
        {percent}
      </Badge>
    ) : status === 'near' ? (
      <Badge>{t('detail.stats.near', { percent })}</Badge>
    ) : (
      <span className="text-micro font-600 text-ink-4">{percent}</span>
    );

  return (
    <>
      <PageHead
        eyebrow={[t('detail.eyebrow'), String(plan.year), currency]}
        title={
          <span className="flex min-w-0 items-center gap-3">
            <PlanChip plan={plan} size="lg" />
            <span className="min-w-0 truncate">{plan.name}</span>
          </span>
        }
        description={plan.note ?? undefined}
        actions={
          <>
            <Button variant="outline" onClick={() => setModal('edit')}>
              <Pencil />
              {t('actions.edit')}
            </Button>
            <Button
              variant="outline"
              onClick={() => setClosed.mutate({ id: plan.id, closed: !closed })}
            >
              {closed ? <RotateCcw /> : <Lock />}
              {closed ? t('actions.reopen') : t('actions.close')}
            </Button>
            <Button variant="danger" onClick={() => setModal('delete')}>
              <Trash2 />
              {t('actions.delete')}
            </Button>
          </>
        }
      />

      {closed && (
        <Alert className="mb-[17px]">
          <Lock />
          <AlertTitle>{t('detail.closed.title')}</AlertTitle>
          <AlertDescription>{t('detail.closed.text')}</AlertDescription>
          <AlertActions>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setClosed.mutate({ id: plan.id, closed: false })}
            >
              {t('actions.reopen')}
            </Button>
          </AlertActions>
        </Alert>
      )}

      <Stats columns={3}>
        <Stat
          label={t('detail.stats.planned')}
          value={money(planned)}
          note={t('detail.stats.plannedNote', { year: plan.year })}
        />
        <Stat
          label={t('detail.stats.spent')}
          aside={spentAside}
          value={<PlanMoney amount={summary.spent} currency={currency} quality={fxQuality} />}
          note={
            paymentCount > 0
              ? `${t('detail.stats.payments', { count: paymentCount })} · ${t(
                  'detail.stats.accounts',
                  { count: accountCount }
                )}`
              : t('detail.stats.noPayments')
          }
        />
        <Stat
          label={over ? t('detail.stats.overBy') : t('detail.stats.left')}
          value={left === null ? '—' : money(Math.abs(left))}
          tone={over ? 'loss' : 'neutral'}
          noteTone="neutral"
          note={over ? t('detail.stats.overNote') : t('detail.stats.leftNote')}
        />
      </Stats>
      <ConversionQualityNote quality={fxQuality} currency={currency} />

      <Card
        className={cn('mb-[18px] px-[21px] pb-4 pt-5', category !== null && 'border-line-hover')}
      >
        <div className="flex items-start justify-between gap-6">
          <div className="min-w-0">
            <CardTitle className="flex flex-wrap items-center gap-2">
              {t('detail.chart.title')}
              <FilterBadge
                labels={categoryLabel ? [categoryLabel] : []}
                onClear={() => setCategory(null)}
                title={t('detail.filterTitle')}
              />
            </CardTitle>
            <CardDescription>
              {categoryLabel
                ? t('detail.chart.subCategory', { category: categoryLabel })
                : t('detail.chart.sub')}
            </CardDescription>
          </div>
          {category === null && (
            <ChartLegend
              className="mt-0 flex-none"
              items={[
                { label: t('detail.chart.month'), swatch: { kind: 'block', color: 'var(--s3)' } },
                { label: t('detail.chart.cumulative'), swatch: { kind: 'line' } },
                { label: t('detail.chart.planned'), swatch: { kind: 'dash' } },
              ]}
            />
          )}
        </div>
        <div className="-mx-1 mt-[18px]">
          <PlanSpendChart
            axis={axis}
            values={bars}
            cumulative={cumulative}
            planned={category === null ? planned : undefined}
            currency={currency}
            year={plan.year}
            currentMonth={currentMonth}
            selectedMonth={month}
            onMonthClick={toggleMonth}
          />
        </div>
      </Card>

      <div className="grid grid-cols-[minmax(300px,0.82fr)_minmax(0,1.5fr)] items-start gap-[18px]">
        <PlanCategoriesCard
          rows={categoryRows}
          currency={currency}
          categoryById={categoryById}
          selected={category}
          onToggle={toggleCategory}
          monthLabel={monthLabel}
          onClearMonth={() => setMonth(null)}
          hasPayments={paymentCount > 0}
        />
        {transactions.isError ? (
          <ReportError onRetry={() => void transactions.refetch()} />
        ) : (
          <PlanPaymentsCard
            planId={plan.id}
            planYear={plan.year}
            closed={closed}
            transactions={all}
            shown={shown}
            accountName={(id) => accountNames.get(id) ?? '—'}
            categoryById={categoryById}
            filterLabels={[monthLabel, categoryLabel].filter((l): l is string => l !== null)}
            onClearFilters={() => {
              setMonth(null);
              setCategory(null);
            }}
            onAddPayments={() => setModal('add')}
          />
        )}
      </div>

      <PlanFormDialog
        open={modal === 'edit'}
        onOpenChange={(open) => setModal(open ? 'edit' : null)}
        plan={plan}
        defaultYear={plan.year}
      />
      <AddPaymentsDialog
        open={modal === 'add'}
        onOpenChange={(open) => setModal(open ? 'add' : null)}
        plan={plan}
      />
      <ConfirmDeleteDialog
        open={modal === 'delete'}
        onOpenChange={(open) => setModal(open ? 'delete' : null)}
        title={t('delete.title', { name: plan.name })}
        description={
          paymentCount > 0
            ? t('delete.withPayments', { count: paymentCount })
            : t('delete.noPayments')
        }
        confirmLabel={t('actions.delete')}
        isPending={deletePlan.isPending}
        onConfirm={() =>
          deletePlan.mutate(plan, {
            onSuccess: () => {
              setModal(null);
              onDeleted(plan.year);
            },
          })
        }
      />
    </>
  );
}
