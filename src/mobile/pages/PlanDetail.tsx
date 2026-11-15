import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useRoute } from 'wouter';
import { ArchiveRestore, Ellipsis, Lock, Pencil, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import type {
  BankTransaction,
  SpendingPlan,
  SpendingPlanDetail as Detail,
  TransactionCategory,
} from '@shared/schema';
import {
  UNCATEGORIZED_KEY,
  planAxisLabels,
  planCategoryRows,
  planMonthAxis,
  planMonthLabel,
  planMonthSpend,
  planRow,
} from '@/utils/spending-plans';
import { utcDayFloor } from '@/utils/chart-axis';
import { useBankTransactionMutations } from '@/hooks/use-bank-account-mutations';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { SPENDING_PLAN_KEYS, useSpendingPlanMutations } from '@/hooks/use-spending-plan-mutations';
import { useSpendingPlanDetail, useSpendingPlanTransactions } from '@/hooks/use-spending-plans';
import { cashflowApi, spendingPlansApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { MissingOrFailed } from '@/mobile/components/LoadError';
import { CashflowHero } from '@/mobile/components/cashflow/CashflowHero';
import { CategoryChip } from '@/mobile/components/cashflow/CategoryChip';
import { ProgressBar } from '@/mobile/components/cashflow/ProgressBar';
import { DayHead } from '@/mobile/components/DayHead';
import { EntityRow, EntityRows } from '@/mobile/components/EntityRow';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { LedgerRow } from '@/mobile/components/LedgerRow';
import { FilterChips } from '@/mobile/components/plans/FilterChip';
import { PlanMonthBars } from '@/mobile/components/plans/PlanMonthBars';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { ScaledConversionNote, ScaledReportError } from '@/mobile/components/Scaled';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useBack } from '@/mobile/hooks/use-back';
import { useRecentCategoryIds } from '@/mobile/hooks/use-categorize-queue';
import { useDeleteAndLeave } from '@/mobile/hooks/use-delete-and-leave';
import { useDeleteDescription } from '@/mobile/hooks/use-delete-description';
import { useRetained } from '@/mobile/hooks/use-retained';
import { dayLabel, groupByDay } from '@/mobile/lib/ledger';
import { categoryShares, planBars, sparseLabels } from '@/mobile/lib/plan-detail';
import { filterPlanPayments } from '@/mobile/lib/plan-rows';
import { NavIconButton, NavTextButton } from '@/mobile/shell/NavBar';
import { planPath, plansPath, routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { DetailSub } from '@/mobile/components/DetailSub';
import { CategoryPickerSheet } from '@/mobile/sheets/CategoryPickerSheet';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { PlanFormSheet } from '@/mobile/sheets/PlanFormSheet';
import { TransactionSheet } from '@/mobile/sheets/TransactionSheet';

/** The card the month bars sit in (the Cashflow bars card). */
const CARD =
  'mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 py-3.5 shadow-e1';
/** Whole amounts in the plan currency, as the plans list writes them. */
const WHOLE = { decimals: 0 } as const;
/** A month name under every third bar (the Cashflow bars' rule). */
const LABEL_EVERY = 3;

/**
 * A spending plan's detail (spec M8–M10, M14, `/cashflow/plans/:id`): the plan's name in the nav
 * bar with `···` (Upravit, Uzavřít / Obnovit plán, Smazat plán), a closed plan's alert, the hero
 * with what is left (or how much it is overspent) in the plan currency and its usage bar, then
 * Útrata po měsících, Kategorie and Platby. A month (a bar) narrows Kategorie and Platby, a
 * category (a row) narrows the bars and Platby; each filter shows as a removable chip above what
 * it narrows, and a second tap clears it.
 *
 * The page owns the delete (its confirm sheet, `useDeleteAndLeave`): the plan is shown as it was
 * until the screen has left for the list.
 */
export default function PlanDetail() {
  const { t } = useTranslation('mobile');
  const { t: tp } = useTranslation('spending_plans');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [, params] = useRoute(routes.planDetail);
  const detail = useSpendingPlanDetail(params?.id);
  const { setClosed } = useSpendingPlanMutations();
  const [sheet, setSheet] = useState<'edit' | 'delete' | null>(null);

  // Not `deletePlan`: it drops the plan's queries while this screen still reads them, which would
  // refetch the deleted plan under it. Here nothing shown is refetched; `useDeleteAndLeave` drops
  // the plan's own queries once the screen has left.
  const removePlan = useMutation({
    mutationFn: (plan: Pick<SpendingPlan, 'id' | 'name'>) => spendingPlansApi.remove(plan.id),
    onSuccess: (_: void, plan) => {
      for (const key of SPENDING_PLAN_KEYS) {
        void queryClient.invalidateQueries({ queryKey: [key], refetchType: 'inactive' });
      }
      toast.success(tp('toast.deleted', { name: plan.name }));
    },
    onError: (error: Error) => {
      toast.error(translateApiError(error, tc));
    },
  });

  const current = detail.data ?? null;
  const kept = useRetained(current);
  const exit = useDeleteAndLeave({
    home: plansPath(kept?.summary.plan.year),
    pathOf: planPath,
    keysOf: (planId) => [
      ['spending-plan', planId],
      ['spending-plan-transactions', planId],
    ],
    remove: (planId) => removePlan.mutateAsync({ id: planId, name: kept?.summary.plan.name ?? '' }),
    closeSheet: () => setSheet(null),
  });
  // A plan being deleted stays on screen (as it was) until the screen has left
  const data = exit.deleting ? kept : current;
  const plan = data?.summary.plan;
  const back = useBack(plansPath(plan?.year));
  const closed = !!plan && plan.closedAt !== null;

  const menu = plan && (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <NavIconButton aria-label={tc('labels.moreActions')}>
          <Ellipsis strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[220px]">
        <RowMenuItem onSelect={() => setSheet('edit')}>
          <Pencil strokeWidth={1.75} aria-hidden />
          {tp('form.editTitle')}
        </RowMenuItem>
        <RowMenuItem onSelect={() => setClosed.mutate({ id: plan.id, closed: !closed })}>
          {closed ? (
            <ArchiveRestore strokeWidth={1.75} aria-hidden />
          ) : (
            <Lock strokeWidth={1.75} aria-hidden />
          )}
          {closed ? tp('actions.reopen') : tp('actions.close')}
        </RowMenuItem>
        <DropdownMenuSeparator />
        <RowMenuItem variant="danger" onSelect={() => setSheet('delete')}>
          <Trash2 strokeWidth={1.75} aria-hidden />
          {tp('actions.delete')}
        </RowMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );

  return (
    <Screen
      title={plan?.name ?? ''}
      large
      leading={
        <NavTextButton
          chevron
          label={t('plans.detail.back', { year: plan?.year ?? new Date().getFullYear() })}
          onClick={back}
        />
      }
      trailing={menu}
    >
      {data && plan ? (
        <>
          {/* A new plan starts without the previous one's filters */}
          <PlanDetailBody
            key={plan.id}
            detail={data}
            onReopen={() => setClosed.mutate({ id: plan.id, closed: false })}
          />
          <PlanFormSheet
            open={sheet === 'edit'}
            onOpenChange={(open) => setSheet(open ? 'edit' : null)}
            plan={plan}
            defaultYear={plan.year}
          />
          <ConfirmSheet
            open={sheet === 'delete'}
            onOpenChange={(open) => setSheet(open ? 'delete' : null)}
            title={tp('delete.title', { name: plan.name })}
            description={
              data.summary.paymentCount > 0
                ? tp('delete.withPayments', { count: data.summary.paymentCount })
                : tp('delete.noPayments')
            }
            confirmLabel={tp('actions.delete')}
            isPending={removePlan.isPending || exit.deleting}
            onConfirm={() => exit.run(plan.id)}
          />
        </>
      ) : detail.isError ? (
        // The backend's "not found" is the plan gone; any other failure is said with a retry
        <MissingOrFailed>{tp('detail.notFound')}</MissingOrFailed>
      ) : (
        <DetailSkeleton />
      )}
    </Screen>
  );
}

interface PlanDetailBodyProps {
  detail: Detail;
  onReopen: () => void;
}

/** The loaded plan: the alert, the hero and the three sections with their filters and sheets. */
function PlanDetailBody({ detail, onReopen }: PlanDetailBodyProps) {
  const { t } = useTranslation('mobile');
  const { t: tp } = useTranslation('spending_plans');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { summary } = detail;
  const { plan, paymentCount, fxQuality } = summary;
  const currency = plan.currency;
  const row = planRow(summary);
  const over = row.left !== null && row.left < 0;
  const transactions = useSpendingPlanTransactions(plan.id);
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const { deleteTransaction } = useBankTransactionMutations();
  const describeDelete = useDeleteDescription();
  const [month, setMonth] = useState<string | null>(null);
  const [category, setCategory] = useState<string | null>(null);

  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);
  const money = (value: number) => fmt.money(value, currency, WHOLE);

  // Útrata po měsících: the whole plan, or one category's spend
  const axis = useMemo(
    () =>
      planMonthAxis(
        plan.year,
        detail.months.map((m) => m.month)
      ),
    [plan.year, detail.months]
  );
  const bars = useMemo(
    () => planBars(axis, planMonthSpend(detail, axis, category), month),
    [detail, axis, category, month]
  );
  const axisLabels = useMemo(
    () => sparseLabels(planAxisLabels(axis, plan.year, fmt.month), LABEL_EVERY),
    [axis, plan.year, fmt.month]
  );
  // Kategorie: the whole plan, or one month's spend
  const categoryRows = useMemo(
    () => categoryShares(planCategoryRows(detail, month)),
    [detail, month]
  );
  const categoryTotal = categoryRows.reduce((sum, r) => sum + r.spent, 0);

  // Platby: newest first, as the backend sends them, narrowed by both filters
  const all = useMemo(() => transactions.data ?? [], [transactions.data]);
  const shown = useMemo(() => filterPlanPayments(all, month, category), [all, month, category]);
  const groups = useMemo(() => groupByDay(shown), [shown]);
  // The sheets' rows (the same one while the picker is open over the transaction sheet)
  const [openTx, setOpenTx] = useState<BankTransaction | null>(null);
  const [pickFor, setPickFor] = useState<BankTransaction | null>(null);
  // The rows' meanings (a refund badge, a transfer chip, the sheet's "Automaticky" line)
  // The open payment stays in the ids after an untagging drops it from the list, so its sheet
  // keeps its "Automaticky" line
  const openTxId = openTx?.id ?? null;
  const ids = useMemo(() => {
    const planIds = all.map((tx) => tx.id);
    return openTxId === null || planIds.includes(openTxId) ? planIds : [...planIds, openTxId];
  }, [all, openTxId]);
  const flows = useQuery({
    queryKey: ['effective-transaction-flows', ids],
    queryFn: () => cashflowApi.getEffectiveFlows(ids),
    enabled: ids.length > 0,
    placeholderData: (previous) => previous,
  });
  const flowById = useMemo(
    () => new Map((flows.data ?? []).map((flow) => [flow.id, flow])),
    [flows.data]
  );

  // A write the sheets make is reported through `patchTx`, since a filtered list may no longer show the row
  const patchTx = (txId: string, patch: Partial<BankTransaction>) => {
    const apply = (tx: BankTransaction | null) => (tx?.id === txId ? { ...tx, ...patch } : tx);
    setOpenTx(apply);
    setPickFor(apply);
  };
  const pickerRow = useRetained(pickFor);
  const recentIds = useRecentCategoryIds(pickerRow?.bankAccountId);
  const [pendingDelete, setPendingDelete] = useState<BankTransaction | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);

  const monthLabel = month === null ? null : planMonthLabel(month, plan.year, fmt.month);
  const categoryLabel = (() => {
    if (category === null) return null;
    if (category === UNCATEGORIZED_KEY) return tp('detail.categories.uncategorized');
    const found = categoryById.get(category);
    return found ? categoryName(found) : '—';
  })();
  const monthChip =
    monthLabel === null ? null : { key: 'month', label: monthLabel, onClear: () => setMonth(null) };
  const categoryChip =
    categoryLabel === null
      ? null
      : { key: 'category', label: categoryLabel, onClear: () => setCategory(null) };
  const toggleMonth = (next: string) => setMonth((m) => (m === next ? null : next));
  const toggleCategory = (next: string) => setCategory((c) => (c === next ? null : next));

  const today = utcDayFloor(Date.now() / 1000);
  const dayText = (day: number) => {
    const label = dayLabel(day, today);
    if ('key' in label) return tc(`time.${label.key}`);
    return fmt.day(label.date, {
      day: 'numeric',
      month: 'short',
      year: label.withYear ? 'numeric' : undefined,
    });
  };

  const percent = row.ratio === null ? null : fmt.percent(row.ratio, 0);
  const spentLine = t('plans.row.spent', {
    spent: row.spent === null ? '—' : money(row.spent),
    planned: money(row.planned),
  });
  const filtered = month !== null || category !== null;
  const emptyLine = (text: string) => (
    <EntityRows>
      <li className="list-none px-3.5 py-4 text-m-sub text-ink-4">{text}</li>
    </EntityRows>
  );

  return (
    <>
      <DetailSub>{[String(plan.year), currency, plan.note].filter(Boolean).join(' · ')}</DetailSub>
      {plan.closedAt !== null && (
        <InlineAlert
          icon={Lock}
          title={tp('detail.closed.title')}
          action={{ label: tp('actions.reopen'), onClick: onReopen }}
        >
          {tp('detail.closed.text')}
        </InlineAlert>
      )}
      <ScaledConversionNote quality={fxQuality} currency={currency} className="mb-3" />
      <CashflowHero
        label={over ? tp('detail.stats.overBy') : tp('detail.stats.left')}
        value={row.left === null ? null : Math.abs(row.left)}
        formatValue={money}
        tone={over ? 'loss' : 'neutral'}
        note={percent === null ? spentLine : `${spentLine} · ${percent}`}
        bar={
          row.ratio === null ? undefined : (
            <ProgressBar value={row.ratio} tone={over ? 'over' : 'limit'} />
          )
        }
      />

      {paymentCount > 0 && (
        <>
          <section>
            <SectionHead title={t('plans.detail.months')} />
            <FilterChips chips={[categoryChip]} />
            <div className={CARD}>
              <PlanMonthBars
                bars={bars}
                axisLabels={axisLabels}
                label={t('plans.detail.months')}
                monthLabel={(m) => planMonthLabel(m, plan.year, fmt.month)}
                selected={month}
                onSelectMonth={toggleMonth}
              />
            </div>
          </section>

          <section>
            <SectionHead
              title={tp('detail.categories.title')}
              note={categoryRows.length > 0 ? money(categoryTotal) : undefined}
            />
            <FilterChips chips={[monthChip]} />
            {categoryRows.length === 0 ? (
              emptyLine(tp('detail.categories.emptyMonth'))
            ) : (
              <EntityRows>
                {categoryRows.map((r) => {
                  const found = r.categoryId ? categoryById.get(r.categoryId) : undefined;
                  return (
                    <PlanCategoryItem
                      key={r.key}
                      category={found ?? null}
                      title={found ? categoryName(found) : tp('detail.categories.uncategorized')}
                      sub={`${tp('detail.categories.payments', { count: r.count })} · ${fmt.percent(r.share, 0)}`}
                      value={money(r.spent)}
                      share={r.share}
                      selected={r.key === category}
                      onToggle={() => toggleCategory(r.key)}
                    />
                  );
                })}
              </EntityRows>
            )}
          </section>
        </>
      )}

      <section>
        <SectionHead
          title={tp('detail.payments.title')}
          note={
            all.length > 0
              ? filtered
                ? tp('detail.payments.filtered', { shown: shown.length, count: all.length })
                : tp('detail.stats.payments', { count: all.length })
              : undefined
          }
        />
        <FilterChips chips={[monthChip, categoryChip]} />
        {transactions.isError ? (
          <ScaledReportError onRetry={() => void transactions.refetch()} />
        ) : transactions.isLoading ? (
          <RowsSkeleton />
        ) : all.length === 0 ? (
          emptyLine(t('plans.detail.paymentsEmpty'))
        ) : shown.length === 0 ? (
          emptyLine(tp('detail.payments.emptyFiltered'))
        ) : (
          groups.map((group) => (
            // One block per day: its head sticks while the day's rows pass under it
            <div key={group.day}>
              <DayHead>{dayText(group.day)}</DayHead>
              <EntityRows>
                {group.rows.map((tx) => (
                  <LedgerRow
                    key={tx.id}
                    tx={tx}
                    category={tx.categoryId ? categoryById.get(tx.categoryId) : undefined}
                    flow={flowById.get(tx.id)}
                    onOpen={() => setOpenTx(tx)}
                    onCategorize={() => setPickFor(tx)}
                    onDelete={() => {
                      setPendingDelete(tx);
                      setDeleteOpen(true);
                    }}
                  />
                ))}
              </EntityRows>
            </div>
          ))
        )}
      </section>

      <TransactionSheet
        tx={openTx}
        open={openTx !== null}
        onOpenChange={(open) => !open && setOpenTx(null)}
        category={categories.find((c) => c.id === openTx?.categoryId)}
        flow={openTx ? flowById.get(openTx.id) : undefined}
        onPickCategory={() => setPickFor(openTx)}
        onChange={(patch) => openTx && patchTx(openTx.id, patch)}
      />
      <CategoryPickerSheet
        tx={pickFor}
        open={pickFor !== null}
        onOpenChange={(open) => !open && setPickFor(null)}
        categories={categories}
        recentIds={recentIds}
        onChange={(patch) => pickFor && patchTx(pickFor.id, patch)}
      />
      <ConfirmSheet
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={tb('confirmDeleteTransaction.title')}
        description={pendingDelete ? describeDelete(pendingDelete) : ''}
        confirmLabel={tb('transaction.delete')}
        onConfirm={() => {
          if (!pendingDelete) return;
          deleteTransaction.mutate(pendingDelete.id, { onSuccess: () => setDeleteOpen(false) });
        }}
        isPending={deleteTransaction.isPending}
      />
    </>
  );
}

interface PlanCategoryItemProps {
  category: TransactionCategory | null;
  title: string;
  /** "3 platby · 34 %". */
  sub: string;
  value: string;
  /** The row's share of the plan's spend, for its bar. */
  share: number;
  /** The category filter is this row: tinted and pressed. */
  selected: boolean;
  onToggle: () => void;
}

/**
 * A category of the plan (spec M9): the round category chip, the name, "N plateb · share", the
 * amount in the plan currency and the share bar. A tap selects the category filter, a second tap
 * clears it; the selected row is tinted (`well-2`) and pressed.
 */
function PlanCategoryItem({
  category,
  title,
  sub,
  value,
  share,
  selected,
  onToggle,
}: PlanCategoryItemProps) {
  return (
    <EntityRow
      className={selected ? 'bg-well-2' : undefined}
      chip={<CategoryChip category={category} />}
      title={title}
      sub={sub}
      value={value}
      bar={<ProgressBar value={share} tone="share" />}
      onClick={onToggle}
      pressed={selected}
    />
  );
}

/** Three rows in outline while the payments load. */
function RowsSkeleton() {
  return (
    <EntityRows>
      {[0, 1, 2].map((i) => (
        <li key={i} aria-hidden className="flex min-h-[60px] list-none items-center gap-3 px-3.5">
          <div className="skeleton size-9 flex-none rounded-full" />
          <div className="min-w-0 flex-1">
            <div className="skeleton h-3.5 w-2/5" />
            <div className="skeleton mt-2 h-3 w-3/5" />
          </div>
        </li>
      ))}
    </EntityRows>
  );
}

/** While the plan loads: the hero and the bars card in outline. */
function DetailSkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 mt-6 rounded-r4 border border-line bg-paper px-4 pb-3 pt-4 shadow-e1">
        <div className="skeleton h-3 w-28" />
        <div className="skeleton mt-3 h-9 w-48" />
        <div className="skeleton mt-3 h-3 w-40" />
        <div className="skeleton mt-3 h-1.5 w-full" />
      </div>
      <div className={CARD}>
        <div className="skeleton h-[110px] w-full" />
      </div>
    </div>
  );
}
