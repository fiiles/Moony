import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Redirect, useParams, useSearch } from 'wouter';
import { Landmark, Tag } from 'lucide-react';
import { toast } from 'sonner';
import type { FlowKind, ReportTransaction } from '@shared/schema';
import { useCategories, useCategoryName } from '@/hooks/use-categories';
import { useCurrency } from '@/lib/currency';
import type { MoneyOptions } from '@/lib/format';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { bankAccountsApi, cashflowApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { movedCount } from '@/utils/report-drilldown';
import { Button } from '@/components/ui/button';
import { ScaledReportError } from '@/mobile/components/Scaled';
import { RowMenu, RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { Money } from '@/mobile/components/Money';
import { usePeriodLabel } from '@/mobile/hooks/use-cashflow';
import { useBack } from '@/mobile/hooks/use-back';
import { cashflowSearch, parseCashflowSearch } from '@/mobile/lib/cashflow-period';
import { DRILLDOWN_PAGE, drilldownRows, type DrilldownRow } from '@/mobile/lib/cashflow-drilldown';
import { categoryTitle } from '@/mobile/lib/cashflow-rows';
import { initials } from '@/mobile/lib/initials';
import { NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { CashflowCategoryPickerSheet } from '@/mobile/sheets/CashflowCategoryPickerSheet';

const FLOWS: readonly FlowKind[] = ['expense', 'income', 'transfer'];

/** An amount with its sign ("− 250,00 Kč", "+ 32 000 Kč"). */
const SIGNED: MoneyOptions = { signed: true };

/**
 * The payments behind one category of the Cashflow report (spec §8.7, `/cashflow/category/<flow>/
 * <key>`): the selected period's list, with the period and the account scope it was pushed with.
 * Each row changes category through the picker — a tap, a swipe to the left and the long press's
 * menu all open it — and the report behind refreshes at once. A row that leaves the category
 * stays in the list, dimmed and marked "přesunuto" (the desktop drilldown's rule, so the list does
 * not reshuffle under the finger).
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

/** The list and the picker of one category; `CashflowCategory` has checked the route's parameters. */
function CategoryDrilldown({ flowKind, categoryKey, search }: CategoryDrilldownProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const { currencyCode, fxRevision } = useCurrency();
  const { categories } = useCategories();
  const categoryName = useCategoryName();
  const state = useMemo(() => parseCashflowSearch(search), [search]);
  const back = useBack(routes.cashflow + cashflowSearch(state));
  const { range, periodLabel } = usePeriodLabel(state);

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
  const [changes, setChanges] = useState<ReadonlyMap<string, string | null>>(new Map());
  const [limit, setLimit] = useState(DRILLDOWN_PAGE);
  // The payment whose category is being chosen, with the category it has now
  const [picking, setPicking] = useState<ReportTransaction | null>(null);

  const rows = snapshot ?? [];
  const moved = movedCount(rows, changes, categoryKey);

  const today = utcDayFloor(Date.now() / 1000);
  const dayLabel = (date: number) => {
    const day = utcDayFloor(date);
    if (day === today) return tc('time.today');
    if (day === today - 86400) return tc('time.yesterday');
    return fmt.day(date, { day: 'numeric', month: 'numeric' });
  };
  const visible = drilldownRows(rows.slice(0, limit), changes, categoryKey, dayLabel);

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
  const summary = [
    periodLabel,
    snapshot && tr('readiness.countedTransactions', { count: rows.length }),
    moved > 0 && tr('readiness.movedCount', { count: moved }),
  ]
    .filter(Boolean)
    .join(' · ');

  // The desktop's order: save, mark the row, refresh the reports behind
  const changeCategory = async (txId: string, categoryId: string) => {
    try {
      await bankAccountsApi.updateTransactionCategory(txId, categoryId);
    } catch (error) {
      toast.error(translateApiError(error as Error, tc));
      throw error;
    }
    setChanges((current) => new Map(current).set(txId, categoryId));
    // The category is saved; a failed refresh must not read as a failed change.
    try {
      await refreshMoneyFlows(queryClient);
    } catch (error) {
      toast.error(translateApiError(error as Error, tc));
    }
  };

  return (
    <Screen
      title={title}
      leading={<NavTextButton chevron label={t('tabs.cashflow')} onClick={back} />}
    >
      <p className="mx-0.5 mb-3.5 text-m-lead text-ink-3">{summary}</p>
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
              <TransactionRow
                key={row.tx.id}
                row={row}
                onChange={(picked) => setPicking({ ...picked.tx, categoryId: picked.categoryId })}
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
      <CashflowCategoryPickerSheet
        tx={picking}
        open={picking !== null}
        onOpenChange={(open) => {
          if (!open) setPicking(null);
        }}
        categories={categories}
        onPicked={changeCategory}
      />
    </Screen>
  );
}

interface TransactionRowProps {
  row: DrilldownRow;
  /** Open the picker for this payment. */
  onChange: (row: DrilldownRow) => void;
}

/**
 * One payment as an entity row: initials chip (green for a credit), who and what, the amount in
 * its own currency with its worth in the report currency under it. A moved row is dimmed and says
 * so. Tap, swipe left and the long press's menu all change the category.
 */
function TransactionRow({ row, onChange }: TransactionRowProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const [menuOpen, setMenuOpen] = useState(false);
  const { tx, credit } = row;
  // The marker leads: the sub-line ends in an ellipsis when it is too long, and "přesunuto" is
  // the one sign of a moved row that is not its dimming
  const sub = (row.moved ? [tr('readiness.moved'), ...row.sub] : row.sub).join(' · ');
  // The converted amount carries the row's own sign (`reportAmount`), not the report line's
  const valueSub =
    row.reportAmount === null ? (
      tr('readiness.fxUnavailable')
    ) : tx.currency !== tx.reportCurrency ? (
      <Money value={row.reportAmount} currency={tx.reportCurrency} options={SIGNED} />
    ) : undefined;

  return (
    <EntityRow
      chip={
        <RowChip tone={credit ? 'gain' : 'default'}>
          {initials(row.title) || <Landmark strokeWidth={1.75} />}
        </RowChip>
      }
      title={row.title}
      sub={sub}
      value={
        <Money
          value={Number(tx.amount) * (credit ? 1 : -1)}
          currency={tx.currency}
          options={SIGNED}
        />
      }
      valueTone={credit ? 'gain' : undefined}
      valueSub={valueSub}
      muted={row.moved}
      onClick={() => onChange(row)}
      onSwipeLeft={() => onChange(row)}
      swipeLeftLabel={t('upkeep.actions.categorize')}
      swipeLeftIcon={Tag}
      onLongPress={() => setMenuOpen(true)}
      accessory={
        <RowMenu
          open={menuOpen}
          onOpenChange={setMenuOpen}
          label={t('cashflow.rowMenu', { title: row.title })}
        >
          <RowMenuItem onSelect={() => onChange(row)}>
            <Tag aria-hidden strokeWidth={1.75} />
            {t('cashflow.picker.change')}
          </RowMenuItem>
        </RowMenu>
      }
    />
  );
}

/** While the list loads for the first time: three rows in outline. */
function DrilldownSkeleton() {
  return (
    <div aria-hidden>
      <EntityRows>
        {[0, 1, 2].map((i) => (
          <li
            key={i}
            className="flex min-h-[60px] list-none items-center gap-3 border-t border-line-soft px-3.5 first:border-t-0"
          >
            <div className="skeleton size-9 flex-none rounded-full" />
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
