import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useSearch } from 'wouter';
import { ChevronLeft, ChevronRight, ClipboardList, Copy, Plus } from 'lucide-react';
import type { SpendingPlan } from '@shared/schema';
import { planRow, type PlanRow } from '@/utils/spending-plans';
import { useSpendingPlansOverview } from '@/hooks/use-spending-plans';
import { useSpendingPlanMutations } from '@/hooks/use-spending-plan-mutations';
import { PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { EmptyState } from '@/components/common/EmptyState';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { StatSkeleton, Stats } from '@/components/common/Stat';
import { PlansStats } from '@/components/spending-plans/PlansStats';
import { PlansTable } from '@/components/spending-plans/PlansTable';
import { PlanFormDialog } from '@/components/spending-plans/PlanFormDialog';
import { TakeOverDialog } from '@/components/spending-plans/TakeOverDialog';
import { ReportError } from '@/components/reports/ReportQualityNote';

type Modal = 'new' | 'edit' | 'delete' | 'takeOver' | null;

/** The year in `?year=` (kept in the URL so the way back from a plan returns to it). */
function yearFromSearch(search: string, fallback: number): number {
  const value = Number(new URLSearchParams(search).get('year'));
  return Number.isInteger(value) && value >= 2000 && value <= 2100 ? value : fallback;
}

/**
 * Výdajové plány (spec §9 "Overview page", prototype): a year's plans — how they are doing and
 * which are over or close to their amount. Four stats, then one table ordered by risk.
 */
export default function SpendingPlans() {
  const { t } = useTranslation('spending_plans');
  const [, setLocation] = useLocation();
  const search = useSearch();
  const currentYear = new Date().getFullYear();
  const year = yearFromSearch(search, currentYear);
  const { data, isLoading, isError, refetch } = useSpendingPlansOverview(year);
  const { setClosed, deletePlan } = useSpendingPlanMutations();
  const [modal, setModal] = useState<Modal>(null);
  const [selected, setSelected] = useState<PlanRow | null>(null);

  // A previous year's response stays shown while this one loads; it still knows the years
  const overview = data?.year === year ? data : undefined;
  const yearsWithPlans = data?.yearsWithPlans ?? [];
  const minYear = Math.min(yearsWithPlans[0] ?? currentYear, currentYear - 1);
  const maxYear = currentYear + 1;
  const prevYearHasPlans = yearsWithPlans.includes(year - 1);
  const rows = useMemo(() => (overview ? overview.plans.map(planRow) : []), [overview]);
  const isEmpty = !!overview && rows.length === 0;
  // The empty year offers "Take over n plans from N−1": count them (the dialog reuses the query)
  const previous = useSpendingPlansOverview(year - 1, isEmpty && prevYearHasPlans);
  const prevYearCount = previous.data?.year === year - 1 ? previous.data.plans.length : null;

  const setYear = (next: number) => setLocation(`/spending-plans?year=${next}`, { replace: true });
  const openPlan = (plan: SpendingPlan) => setLocation(`/spending-plans/${plan.id}`);
  const close = () => {
    setModal(null);
    setSelected(null);
  };
  const closeIf = (open: boolean) => {
    if (!open) close();
  };
  const editPlan = (plan: SpendingPlan) => {
    setSelected(rows.find((r) => r.summary.plan.id === plan.id) ?? null);
    setModal('edit');
  };

  const newPlanButton = (primary: boolean) => (
    <Button variant={primary ? 'default' : 'outline'} onClick={() => setModal('new')}>
      <Plus />
      {t('actions.newPlan')}
    </Button>
  );
  const takeOverButton = (primary: boolean, label: string) => (
    <Button variant={primary ? 'default' : 'outline'} onClick={() => setModal('takeOver')}>
      <Copy />
      {label}
    </Button>
  );

  const yearStepper = (
    <div className="mx-1 inline-flex items-center gap-1" role="group" aria-label={t('year.label')}>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={t('year.prev')}
        disabled={year <= minYear}
        onClick={() => setYear(year - 1)}
      >
        <ChevronLeft />
      </Button>
      <b className="min-w-[48px] text-center text-body font-650 text-ink num">{year}</b>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={t('year.next')}
        disabled={year >= maxYear}
        onClick={() => setYear(year + 1)}
      >
        <ChevronRight />
      </Button>
    </div>
  );

  const deleteRow = selected?.summary;

  return (
    <>
      <PageHead
        eyebrow={t('eyebrow')}
        title={t('title')}
        description={t('subtitle')}
        actions={
          <>
            {yearStepper}
            {rows.length > 0 &&
              prevYearHasPlans &&
              takeOverButton(false, t('actions.takeOver', { year: year - 1 }))}
            {newPlanButton(true)}
          </>
        }
      />

      {isError && !overview ? (
        <ReportError onRetry={() => void refetch()} />
      ) : isLoading || !overview ? (
        <Stats>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      ) : isEmpty ? (
        <Card variant="table">
          <EmptyState
            bare
            icon={<ClipboardList />}
            title={t('empty.title', { year })}
            description={
              prevYearHasPlans
                ? t('empty.descriptionTakeOver', { year: year - 1 })
                : t('empty.description')
            }
            action={
              prevYearHasPlans ? (
                <div className="flex flex-wrap justify-center gap-2">
                  {takeOverButton(
                    true,
                    prevYearCount === null
                      ? t('actions.takeOver', { year: year - 1 })
                      : t('empty.takeOver', { count: prevYearCount, year: year - 1 })
                  )}
                  {newPlanButton(false)}
                </div>
              ) : (
                newPlanButton(true)
              )
            }
          />
        </Card>
      ) : (
        <>
          <PlansStats overview={overview} rows={rows} />
          <PlansTable
            year={year}
            rows={rows}
            currency={overview.currency}
            onOpen={openPlan}
            onEdit={editPlan}
            onToggleClosed={(plan) =>
              setClosed.mutate({ id: plan.id, closed: plan.closedAt === null })
            }
            onDelete={(row) => {
              setSelected(row);
              setModal('delete');
            }}
          />
        </>
      )}

      <PlanFormDialog
        open={modal === 'new'}
        onOpenChange={closeIf}
        defaultYear={year}
        onCreated={openPlan}
      />
      <PlanFormDialog
        open={modal === 'edit'}
        onOpenChange={closeIf}
        plan={selected?.summary.plan ?? null}
        defaultYear={year}
      />
      <TakeOverDialog
        open={modal === 'takeOver'}
        onOpenChange={closeIf}
        fromYear={year - 1}
        toYear={year}
      />
      <ConfirmDeleteDialog
        open={modal === 'delete'}
        onOpenChange={closeIf}
        title={t('delete.title', { name: deleteRow?.plan.name ?? '' })}
        description={
          deleteRow && deleteRow.paymentCount > 0
            ? t('delete.withPayments', { count: deleteRow.paymentCount })
            : t('delete.noPayments')
        }
        confirmLabel={t('actions.delete')}
        isPending={deletePlan.isPending}
        onConfirm={() => deleteRow && deletePlan.mutate(deleteRow.plan, { onSuccess: close })}
      />
    </>
  );
}
