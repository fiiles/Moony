import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useSearch } from 'wouter';
import { ClipboardList, Copy, Plus } from 'lucide-react';
import { toast } from 'sonner';
import type { SpendingPlan } from '@shared/schema';
import { planRow, type PlanRow } from '@/utils/spending-plans';
import { useSpendingPlanMutations } from '@/hooks/use-spending-plan-mutations';
import { useSpendingPlansOverview } from '@/hooks/use-spending-plans';
import { useFormat } from '@/lib/use-format';
import { Button } from '@/components/ui/button';
import { CashflowHero } from '@/mobile/components/cashflow/CashflowHero';
import { EntityRows } from '@/mobile/components/EntityRow';
import { PlanRowItem } from '@/mobile/components/plans/PlanRowItem';
import { yearSpentLine } from '@/mobile/components/plans/plan-text';
import { TakeOverSheet } from '@/mobile/components/plans/TakeOverSheet';
import { YearBar } from '@/mobile/components/plans/YearBar';
import {
  ScaledConversionNote,
  ScaledEmptyState,
  ScaledReportError,
} from '@/mobile/components/Scaled';
import { SearchField } from '@/mobile/components/SearchField';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useBack } from '@/mobile/hooks/use-back';
import {
  plansCardSummary,
  planListSections,
  SEARCH_FROM,
  takeOverInserts,
} from '@/mobile/lib/plan-rows';
import { NavIconButton, NavTextButton } from '@/mobile/shell/NavBar';
import { planPath, plansPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { PlanFormSheet } from '@/mobile/sheets/PlanFormSheet';

/** The year in `?year=` (the desktop's rule: a whole year in 2000–2100, else the current one). */
function yearFromSearch(search: string, fallback: number): number {
  const value = Number(new URLSearchParams(search).get('year'));
  return Number.isInteger(value) && value >= 2000 && value <= 2100 ? value : fallback;
}

const NEW_PLAN_BUTTON =
  'h-auto min-h-12 whitespace-normal rounded-[10px] px-5 py-2 text-center text-m-row';

/**
 * Výdajové plány (spec M4–M7, pushed on the Cashflow tab): a year's plans — the hero with what is
 * left in the active plans and the year's spend, then the active plans in the risk order and the
 * closed ones dimmed below. `+` adds a plan (its detail opens), a long press on a row opens its
 * menu. An empty year offers the take-over of last year's open plans and "Nový plán". The year
 * lives in `?year=`, so the way back from a plan returns to it.
 */
export default function Plans() {
  const { t } = useTranslation('mobile');
  const { t: tp } = useTranslation('spending_plans');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const [, setLocation] = useLocation();
  const search = useSearch();
  const back = useBack(routes.cashflow);
  const currentYear = new Date().getFullYear();
  const year = yearFromSearch(search, currentYear);
  const { data, isLoading, isError, refetch } = useSpendingPlansOverview(year);
  const { setClosed, deletePlan, createPlans } = useSpendingPlanMutations();
  const [query, setQuery] = useState('');
  const [formOpen, setFormOpen] = useState(false);
  const [formPlan, setFormPlan] = useState<SpendingPlan | null>(null);
  const [deleting, setDeleting] = useState<PlanRow | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [takeOverOpen, setTakeOverOpen] = useState(false);

  // A previous year's response stays shown while this one loads; it still knows the years
  const overview = data?.year === year ? data : undefined;
  const yearsWithPlans = data?.yearsWithPlans ?? [];
  const minYear = Math.min(yearsWithPlans[0] ?? currentYear, currentYear - 1);
  const maxYear = currentYear + 1;
  const prevYearHasPlans = yearsWithPlans.includes(year - 1);
  const rows = useMemo(() => (overview ? overview.plans.map(planRow) : []), [overview]);
  const isEmpty = !!overview && rows.length === 0;
  const summary = useMemo(() => (overview ? plansCardSummary(overview) : null), [overview]);
  const sections = useMemo(
    () => planListSections(rows, query, fmt.locale),
    [rows, query, fmt.locale]
  );
  // The empty year offers "Převzít n plánů z roku N−1": the plans a take-over would create
  const previous = useSpendingPlansOverview(year - 1, isEmpty && prevYearHasPlans);
  const inserts = useMemo(
    () =>
      previous.data?.year === year - 1
        ? takeOverInserts(
            previous.data.plans.map((s) => s.plan),
            [],
            year,
            fmt.locale
          )
        : [],
    [previous.data, year, fmt.locale]
  );

  const setYear = (next: number) => {
    setQuery('');
    setLocation(plansPath(next), { replace: true });
  };
  const newPlan = () => {
    setFormPlan(null);
    setFormOpen(true);
  };
  const editPlan = (plan: SpendingPlan) => {
    setFormPlan(plan);
    setFormOpen(true);
  };
  const takeOver = () =>
    createPlans.mutate(inserts, {
      onSuccess: (created) => {
        toast.success(tp('toast.takenOver', { count: created.length, year: year - 1 }));
        setTakeOverOpen(false);
      },
    });

  const searching = query.trim() !== '';
  const noMatch = searching && sections.active.length === 0 && sections.closed.length === 0;
  const money = (value: number, currency: string) => fmt.money(value, currency, { decimals: 0 });

  let body;
  if (isError && !overview) {
    body = <ScaledReportError onRetry={() => void refetch()} />;
  } else if (isLoading || !overview) {
    body = <PlansSkeleton />;
  } else if (isEmpty) {
    body = (
      <ScaledEmptyState
        bare
        icon={<ClipboardList />}
        title={tp('empty.title', { year })}
        description={
          inserts.length > 0
            ? tp('empty.descriptionTakeOver', { year: year - 1 })
            : tp('empty.description')
        }
        className="px-5 py-8"
        action={
          <div className="flex w-full flex-col items-stretch gap-2">
            {inserts.length > 0 && (
              <Button className={NEW_PLAN_BUTTON} onClick={() => setTakeOverOpen(true)}>
                <Copy />
                {tp('empty.takeOver', { count: inserts.length, year: year - 1 })}
              </Button>
            )}
            <Button
              variant={inserts.length > 0 ? 'outline' : 'default'}
              className={NEW_PLAN_BUTTON}
              onClick={newPlan}
            >
              <Plus />
              {tp('actions.newPlan')}
            </Button>
          </div>
        }
      />
    );
  } else {
    const item = (row: PlanRow) => (
      <PlanRowItem
        key={row.summary.plan.id}
        row={row}
        onEdit={editPlan}
        onToggleClosed={(plan) => setClosed.mutate({ id: plan.id, closed: plan.closedAt === null })}
        onDelete={(target) => {
          setDeleting(target);
          setDeleteOpen(true);
        }}
      />
    );
    body = (
      <>
        {summary && (
          <>
            <ScaledConversionNote
              quality={overview.fxQuality}
              currency={overview.currency}
              className="mb-3"
            />
            <CashflowHero
              label={tp('stats.remaining')}
              value={summary.left}
              formatValue={(value) => money(value, summary.currency)}
              tone={summary.left !== null && summary.left < 0 ? 'loss' : 'neutral'}
              note={yearSpentLine(t, fmt.money, summary)}
            />
          </>
        )}
        {rows.length > SEARCH_FROM && (
          <SearchField
            value={query}
            onChange={setQuery}
            placeholder={tp('table.search')}
            clearLabel={tc('a11y.clearSearch')}
          />
        )}
        {noMatch && (
          <p className="mx-0.5 my-4 text-center text-m-sub text-ink-4">
            {tp('table.noResults.searchAll', { query: query.trim(), year })}
          </p>
        )}
        {sections.active.length > 0 && (
          <section>
            <SectionHead
              title={t('plans.sections.active')}
              note={tp('stats.activePlans', { count: sections.active.length })}
            />
            <EntityRows>{sections.active.map(item)}</EntityRows>
          </section>
        )}
        {sections.closed.length > 0 && (
          <section>
            <SectionHead
              title={t('plans.sections.closed')}
              note={tp('stats.closed', { count: sections.closed.length })}
            />
            <EntityRows>{sections.closed.map(item)}</EntityRows>
          </section>
        )}
      </>
    );
  }

  const deletingPlan = deleting?.summary;

  return (
    <Screen
      title={tp('title')}
      leading={<NavTextButton chevron label={t('tabs.cashflow')} onClick={back} />}
      trailing={
        <NavIconButton aria-label={tp('actions.newPlan')} onClick={newPlan}>
          <Plus strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      }
    >
      <YearBar year={year} minYear={minYear} maxYear={maxYear} onStep={setYear} />
      {body}
      <PlanFormSheet
        open={formOpen}
        onOpenChange={setFormOpen}
        plan={formPlan}
        defaultYear={year}
        onSaved={formPlan ? undefined : (plan) => setLocation(planPath(plan.id))}
      />
      <ConfirmSheet
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={tp('delete.title', { name: deletingPlan?.plan.name ?? '' })}
        description={
          deletingPlan && deletingPlan.paymentCount > 0
            ? tp('delete.withPayments', { count: deletingPlan.paymentCount })
            : tp('delete.noPayments')
        }
        confirmLabel={tp('actions.delete')}
        isPending={deletePlan.isPending}
        onConfirm={() =>
          deletingPlan &&
          deletePlan.mutate(deletingPlan.plan, { onSuccess: () => setDeleteOpen(false) })
        }
      />
      <TakeOverSheet
        open={takeOverOpen}
        onOpenChange={setTakeOverOpen}
        count={inserts.length}
        fromYear={year - 1}
        isPending={createPlans.isPending}
        onConfirm={takeOver}
      />
    </Screen>
  );
}

/** While a year loads for the first time: the hero and a few rows in outline. */
function PlansSkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 rounded-r4 border border-line bg-paper px-4 pb-3 pt-4 shadow-e1">
        <div className="skeleton h-3 w-28" />
        <div className="skeleton mt-3 h-9 w-48" />
        <div className="skeleton mt-3 h-3 w-40" />
      </div>
      <EntityRows>
        {[0, 1, 2].map((i) => (
          <li
            key={i}
            className="flex min-h-[60px] list-none items-center gap-3 border-t border-line-soft px-3.5 first:border-t-0"
          >
            <div className="skeleton size-9 flex-none rounded-[10px]" />
            <div className="min-w-0 flex-1">
              <div className="skeleton h-3.5 w-2/5" />
              <div className="skeleton mt-2 h-3 w-3/5" />
            </div>
          </li>
        ))}
      </EntityRows>
    </div>
  );
}
