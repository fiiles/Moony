import { useCallback, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Redirect, useParams, useSearch } from 'wouter';
import { toast } from 'sonner';
import type {
  BankTransaction,
  EffectiveTransactionFlow,
  FlowKind,
  ReportTransaction,
} from '@shared/schema';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { getCompleteBankHistory } from '@/lib/bank-history';
import { useCurrency } from '@/lib/currency';
import { cashflowApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { applyBulkChanges, restoreBulkChanges } from '@/utils/report-drilldown';
import { Button } from '@/components/ui/button';
import { ScaledReportError } from '@/mobile/components/Scaled';
import {
  DrilldownSkeleton,
  DrilldownTransactionRow,
} from '@/mobile/components/cashflow/DrilldownRow';
import { EntityRows } from '@/mobile/components/EntityRow';
import { useCashflowReport, usePeriodLabel } from '@/mobile/hooks/use-cashflow';
import { useTransactionDay } from '@/mobile/hooks/use-transaction-day';
import { useBack } from '@/mobile/hooks/use-back';
import { useRecentCategoryIds } from '@/mobile/hooks/use-categorize-queue';
import { useRetained } from '@/mobile/hooks/use-retained';
import { cashflowSearch, parseCashflowSearch } from '@/mobile/lib/cashflow-period';
import {
  DRILLDOWN_PAGE,
  drilldownRows,
  monthlyTotals,
  TREND_MONTHS,
  trendMonths,
} from '@/mobile/lib/cashflow-drilldown';
import { expenseRowMoney, limitStatus } from '@/mobile/lib/budget-rows';
import { CashflowHero } from '@/mobile/components/cashflow/CashflowHero';
import { CategoryTrend } from '@/mobile/components/cashflow/CategoryTrend';
import { ProgressBar } from '@/mobile/components/cashflow/ProgressBar';
import { SectionHead } from '@/mobile/components/SectionHead';
import { useSummaryMoney } from '@/mobile/hooks/use-summary-money';
import { accountScope, categoryTitle } from '@/mobile/lib/cashflow-rows';
import { DetailSub } from '@/mobile/components/DetailSub';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { CategoryPickerSheet } from '@/mobile/sheets/CategoryPickerSheet';
import { TransactionSheet } from '@/mobile/sheets/TransactionSheet';

const FLOWS: readonly FlowKind[] = ['expense', 'income', 'transfer'];

/**
 * The payments behind one category of the Cashflow report (spec §8.7, `/cashflow/category/<flow>/
 * <key>`): the selected period's list, with the period and the account scope it was pushed with.
 * The category's name is the large title (README §13, "Detail screens", C10), with the flow and the
 * account scope under it. It opens with what the category came to (audit 2026-11-15 D19): the hero with the period's
 * total, the limit's state in one sentence and its bar (an expense category with a limit), the
 * small six-month trend (`CategoryTrend`), then the payments under a head that counts them.
 * A tap on a row opens its transaction sheet (the category and the meaning of the movement can be
 * changed there, the payment deleted); a swipe to the left and the long press's menu open the
 * category picker of Finance directly. The report behind refreshes at once. A row that leaves the
 * category or the flow stays in the list, dimmed and marked "přesunuto" (the desktop drilldown's
 * rule, so the list does not reshuffle under the finger); a deleted payment leaves it.
 */
export default function CashflowCategory() {
  const { flow, key } = useParams<{ flow: string; key: string }>();
  const search = useSearch();
  const flowKind = FLOWS.find((candidate) => candidate === flow);
  if (!flowKind || !key) {
    return <Redirect replace to={routes.cashflow + cashflowSearch(parseCashflowSearch(search))} />;
  }
  // A new category or period is a new list: its snapshot and changes start over
  return (
    <CategoryDrilldown
      key={`${flowKind}/${key}?${search}`}
      flowKind={flowKind}
      categoryKey={key}
      search={search}
    />
  );
}

interface CategoryDrilldownProps {
  flowKind: FlowKind;
  /** Category id, or `uncategorized`. */
  categoryKey: string;
  /** The period search of the Cashflow tab this was pushed from (without the `?`). */
  search: string;
}

/** The row as the sheets show it: the category changed here wins over the one it was loaded with. */
function withChange(
  tx: BankTransaction | null,
  changes: ReadonlyMap<string, string | null>
): BankTransaction | null {
  return tx && changes.has(tx.id) ? { ...tx, categoryId: changes.get(tx.id) ?? null } : tx;
}

/** Without a label: the moved count of the whole snapshot never shows a day. */
const NO_DAY = () => '';

/** The list and the sheets of one category; `CashflowCategory` has checked the route's parameters. */
function CategoryDrilldown({ flowKind, categoryKey, search }: CategoryDrilldownProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const { t: tb } = useTranslation('budgeting');
  const queryClient = useQueryClient();
  const { currencyCode, fxRevision } = useCurrency();
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const state = useMemo(() => parseCashflowSearch(search), [search]);
  const back = useBack(routes.cashflow + cashflowSearch(state));
  const { range, periodLabel } = usePeriodLabel(state);
  const { accounts } = useBankAccounts();
  const scope = accountScope(state.accountIds, accounts);

  // Outside the money-flow refresh keys and dropped on leaving, so a recategorization never
  // reshuffles the open list (the desktop dialog's query, key for key).
  const { data, isError, refetch } = useQuery({
    queryKey: [
      'report-drilldown',
      { flowKind, categoryId: categoryKey },
      range.start,
      range.end,
      state.accountIds,
      currencyCode,
      fxRevision,
    ],
    queryFn: () =>
      cashflowApi.getTransactions(range.start, range.end, {
        accountIds: state.accountIds,
        flowKind,
        categoryId: categoryKey,
        reportCurrency: currencyCode,
      }),
    gcTime: 0,
    staleTime: Infinity,
  });
  // The first response is the list; what changes afterwards lies on top of it
  const [snapshot, setSnapshot] = useState<ReportTransaction[] | null>(null);
  if (snapshot === null && data) setSnapshot(data);
  // The category picks made here (a pick, and the other payments a remembered pick took along)
  const [changes, setChanges] = useState<ReadonlyMap<string, string | null>>(new Map());
  // The effective flow the transaction sheet read for the payments it was opened on
  const [flows, setFlows] = useState<ReadonlyMap<string, FlowKind>>(new Map());
  // Payments deleted in their sheet, or no longer in the account: they leave the list
  const [removed, setRemoved] = useState<ReadonlySet<string>>(new Set());
  const [limit, setLimit] = useState(DRILLDOWN_PAGE);

  // The sheets work on the account's own row (`BankTransaction`), which the report's row lacks
  const [detailsFor, setDetailsFor] = useState<BankTransaction | null>(null);
  const [pickFor, setPickFor] = useState<BankTransaction | null>(null);
  const detailsTx = useMemo(() => withChange(detailsFor, changes), [detailsFor, changes]);
  const pickTx = useMemo(() => withChange(pickFor, changes), [pickFor, changes]);
  // The sheets keep their row while they slide away, and so does what is read for it
  const detailsRow = useRetained(detailsTx);
  const flow = useQuery({
    queryKey: ['effective-transaction-flows', [detailsRow?.id]],
    queryFn: () => cashflowApi.getEffectiveFlows([detailsRow?.id as string]),
    enabled: detailsRow !== null,
  });
  const recordFlow = useCallback(
    (flow: EffectiveTransactionFlow) =>
      setFlows((current) =>
        current.get(flow.id) === flow.effectiveFlowKind
          ? current
          : new Map(current).set(flow.id, flow.effectiveFlowKind)
      ),
    []
  );
  // Whenever the meaning is read (a change of it refetches the query) the row follows it
  const read = flow.data?.[0];
  if (read && flows.get(read.id) !== read.effectiveFlowKind) recordFlow(read);
  // A category carries a role: after a pick the row's meaning is read again, which also gives
  // back what a row seen in another category earlier had recorded. A failed read leaves the
  // category rule to mark the row.
  const readFlowAgain = async (id: string) => {
    try {
      const [again] = await queryClient.fetchQuery({
        queryKey: ['effective-transaction-flows', [id]],
        queryFn: () => cashflowApi.getEffectiveFlows([id]),
      });
      if (again) recordFlow(again);
    } catch {
      /* nothing to say: the list only loses a mark */
    }
  };
  const pickerRow = useRetained(pickTx);
  const recentIds = useRecentCategoryIds(pickerRow?.bankAccountId);

  const original = useMemo(() => snapshot ?? [], [snapshot]);
  const rows = useMemo(() => original.filter((row) => !removed.has(row.id)), [original, removed]);
  const flowRule = useMemo(() => ({ flowKind, current: flows }), [flowKind, flows]);
  // The whole snapshot is judged, the list shows its first `limit`
  const moved = useMemo(
    () =>
      drilldownRows(rows, changes, categoryKey, NO_DAY, flowRule).filter((row) => row.moved).length,
    [rows, changes, categoryKey, flowRule]
  );

  // A payment's day as every list of payments words it (C11)
  const dayLabel = useTransactionDay();
  const visible = drilldownRows(rows.slice(0, limit), changes, categoryKey, dayLabel, flowRule);

  const title = categoryTitle(
    categoryKey,
    flowKind,
    categories.find((category) => category.id === categoryKey),
    '',
    {
      categoryName,
      uncategorized: tr('actuals.uncategorized'),
      uncategorizedTransfers: tr('actuals.uncategorizedTransfers'),
    }
  );
  // The hero (D19): what the category came to in the period — the report's own line, as on the
  // tab — with the limit's state and its bar for an expense category that has one
  const report = useCashflowReport(state);
  const { money, nativeMoney } = useSummaryMoney(report.reportCurrency);
  const expenseRow =
    flowKind === 'expense' ? report.expenses.find((row) => row.key === categoryKey) : undefined;
  const group =
    flowKind === 'income'
      ? report.actuals?.incomeCategories.find((g) => g.key === categoryKey)
      : flowKind === 'transfer'
        ? report.actuals?.transfers.find((g) => g.key === categoryKey)
        : undefined;
  const total = !report.actuals
    ? undefined
    : expenseRow
      ? expenseRow.amountUnavailable
        ? null
        : expenseRow.amount
      : group
        ? Number(group.amount) || 0
        : 0;
  const rowLimit = expenseRow?.limit ?? null;
  const status =
    rowLimit && expenseRow
      ? limitStatus(rowLimit, expenseRowMoney(expenseRow, report.reportCurrency).limitAmounts)
      : null;
  const limitNote =
    status === null
      ? expenseRow && !expenseRow.hasLimit
        ? tb('noBudget')
        : undefined
      : status.kind === 'unavailable'
        ? tb('readiness.usageUnavailable')
        : t(`cashflow.limit.${status.kind}`, {
            amount: nativeMoney(status.amount, status.currency),
            limit: nativeMoney(status.limit, status.currency),
          });

  // The small trend: the category's months up to the period's last one
  const months = useMemo(() => trendMonths(range.end, TREND_MONTHS), [range.end]);
  const trendQuery = useQuery({
    queryKey: [
      'report-drilldown-trend',
      { flowKind, categoryId: categoryKey },
      months[0],
      range.end,
      state.accountIds,
      currencyCode,
      fxRevision,
    ],
    queryFn: () =>
      cashflowApi.getTransactions(months[0], range.end, {
        accountIds: state.accountIds,
        flowKind,
        categoryId: categoryKey,
        reportCurrency: currencyCode,
      }),
    gcTime: 0,
    staleTime: Infinity,
  });
  const trend = useMemo(
    () => (trendQuery.data ? monthlyTotals(trendQuery.data, months) : null),
    [trendQuery.data, months]
  );

  // The newest tap wins: a row opened while another is still being read replaces it
  const opening = useRef(0);
  /**
   * The payment's own row, from the account's complete history (the entry the picker's recents
   * read, so they share it). A payment that is not there any more was deleted elsewhere: it
   * leaves the list and nothing opens.
   */
  const openFor = async (row: ReportTransaction, open: (tx: BankTransaction) => void) => {
    const mine = ++opening.current;
    try {
      const history = await queryClient.fetchQuery({
        queryKey: ['bank-transactions', row.bankAccountId, 'history'],
        queryFn: () => getCompleteBankHistory(row.bankAccountId),
      });
      if (opening.current !== mine) return;
      const found = history?.transactions.find((tx) => tx.id === row.id);
      if (found) open(found);
      else setRemoved((current) => new Set(current).add(row.id));
    } catch (error) {
      if (opening.current === mine) toast.error(translateApiError(error as Error, tc));
    }
  };

  return (
    <Screen
      title={title}
      large
      leading={<NavTextButton chevron label={t('tabs.cashflow')} onClick={back} />}
    >
      {/* What the category is, under its large title (README §13, "Detail screens"): the flow
          it belongs to and whose accounts the list covers */}
      <DetailSub>
        {flowKind === 'expense'
          ? tr('sections.expenses')
          : flowKind === 'income'
            ? tr('sections.income')
            : tr('actuals.transfers.title')}
        {' · '}
        {scope === null ? t('cashflow.scopeAll') : t('cashflow.scope', { count: scope })}
      </DetailSub>
      {total !== undefined && (
        <CashflowHero
          label={periodLabel}
          value={total}
          formatValue={money}
          tone="neutral"
          note={
            limitNote && (
              <span className={status?.kind === 'over' ? 'text-loss' : undefined}>{limitNote}</span>
            )
          }
          bar={
            rowLimit && status && status.kind !== 'unavailable' ? (
              <ProgressBar
                value={(rowLimit.percentage ?? 0) / 100}
                tone={status.kind === 'over' ? 'over' : 'limit'}
              />
            ) : undefined
          }
        />
      )}
      {trend && trend.some((value) => value !== 0) && (
        <CategoryTrend
          months={months}
          totals={trend}
          selected={(month) => month >= range.start && month <= range.end}
          formatValue={money}
        />
      )}
      {snapshot !== null && rows.length > 0 && (
        <SectionHead
          title={tr('readiness.countedTransactions', { count: rows.length })}
          note={moved > 0 ? tr('readiness.movedCount', { count: moved }) : undefined}
        />
      )}
      {snapshot === null ? (
        isError ? (
          <ScaledReportError onRetry={() => void refetch()} />
        ) : (
          <DrilldownSkeleton />
        )
      ) : rows.length === 0 ? (
        <EntityRows>
          <li className="list-none px-3.5 py-4 text-m-sub text-ink-4">
            {tr('actuals.breakdown.empty')}
          </li>
        </EntityRows>
      ) : (
        <>
          <EntityRows>
            {visible.map((row) => (
              <DrilldownTransactionRow
                key={row.tx.id}
                row={row}
                onOpen={(picked) => void openFor(picked.tx, setDetailsFor)}
                onCategorize={(picked) => void openFor(picked.tx, setPickFor)}
              />
            ))}
          </EntityRows>
          {rows.length > limit && (
            <Button
              variant="secondary"
              className="h-auto min-h-12 whitespace-normal py-2 text-center w-full rounded-[10px] text-m-row"
              onClick={() => setLimit(limit + DRILLDOWN_PAGE)}
            >
              {t('cashflow.showMore', { count: Math.min(DRILLDOWN_PAGE, rows.length - limit) })}
            </Button>
          )}
        </>
      )}
      <TransactionSheet
        tx={detailsTx}
        open={detailsTx !== null}
        onOpenChange={(next) => !next && setDetailsFor(null)}
        category={categories.find((category) => category.id === detailsRow?.categoryId)}
        flow={read}
        onPickCategory={() => setPickFor(detailsFor)}
        onChange={(patch) => {
          if (!detailsFor) return;
          const id = detailsFor.id;
          setDetailsFor((current) => (current?.id === id ? { ...current, ...patch } : current));
        }}
        onDeleted={() => detailsRow && setRemoved((current) => new Set(current).add(detailsRow.id))}
      />
      <CategoryPickerSheet
        tx={pickTx}
        open={pickTx !== null}
        onOpenChange={(next) => !next && setPickFor(null)}
        categories={categories}
        recentIds={recentIds}
        onChange={(patch) => {
          if (!pickFor) return;
          const id = pickFor.id;
          const { categoryId, ...rest } = patch;
          if (typeof categoryId === 'string') {
            setChanges((current) => new Map(current).set(id, categoryId));
            void readFlowAgain(id);
          }
          // What else the picker changed (a declined suggestion) follows on the copies the sheets hold
          if (Object.keys(rest).length > 0) {
            setPickFor((current) => (current?.id === id ? { ...current, ...rest } : current));
            setDetailsFor((current) => (current?.id === id ? { ...current, ...rest } : current));
          }
        }}
        onHistoryApplied={(changed, categoryId) =>
          setChanges((current) => applyBulkChanges(current, original, changed, categoryId))
        }
        onHistoryRestored={(changed) =>
          setChanges((current) => restoreBulkChanges(current, original, changed))
        }
      />
    </Screen>
  );
}
