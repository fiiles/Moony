import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useRoute, useSearch } from 'wouter';
import { CalendarSearch, Ellipsis, ListFilter, Pencil, Upload, Wallet } from 'lucide-react';
import { toast } from 'sonner';
import type {
  BankAccountWithInstitution,
  BankBalanceRecord,
  BankTransaction,
  SavingsAccountZone,
} from '@shared/schema';
import { useCategories } from '@/hooks/use-categories';
import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { useFormat } from '@/lib/use-format';
import { calculateZonedInterest } from '@/utils/bank-account-zones';
import {
  hasRecentRows,
  monthLabelDate,
  monthOf,
  monthlyFlows,
  newestMonthWithRows,
  recentWindowStart,
  summarizeFlow,
} from '@/utils/bank-activity';
import { averageMonthlyChange, changeThisYear, newestRecord } from '@/utils/balance-records';
import { utcDayFloor } from '@/utils/chart-axis';
import type { EventCluster } from '@/utils/chart-scale';
import { completeAccountIdentifier } from '@/utils/iban-utils';
import { CHART_PERIODS, chartPeriodStart, type ChartPeriod } from '@/utils/period';
import { EmptyState } from '@/components/common/EmptyState';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { ButtonPair } from '@/mobile/components/ButtonPair';
import { DayHead } from '@/mobile/components/DayHead';
import { EntityRows } from '@/mobile/components/EntityRow';
import { FlowStrip } from '@/mobile/components/FlowStrip';
import { Hero } from '@/mobile/components/Hero';
import { LedgerRow } from '@/mobile/components/LedgerRow';
import { SearchField } from '@/mobile/components/SearchField';
import { SectionHead } from '@/mobile/components/SectionHead';
import { Segment } from '@/mobile/components/Segment';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { useAccountDetail } from '@/mobile/hooks/use-account-detail';
import { useBack } from '@/mobile/hooks/use-back';
import { useDeleteDescription } from '@/mobile/hooks/use-delete-description';
import { useImportSheet } from '@/mobile/hooks/use-import-sheet';
import { useLedger, type Ledger } from '@/mobile/hooks/use-ledger';
import { useLedgerMutations } from '@/mobile/hooks/use-ledger-mutations';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { balanceLine } from '@/mobile/lib/balance-line';
import { recentCategoryIds } from '@/mobile/lib/category-pick';
import { cashChange } from '@/mobile/lib/finance-rows';
import {
  activeFilterCount,
  dayLabel,
  DEFAULT_LEDGER,
  groupByDay,
  periodForMonth,
  type LedgerKind,
  type LedgerState,
} from '@/mobile/lib/ledger';
import { NavIconButton, NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';
import { BalanceHistorySheet } from '@/mobile/sheets/BalanceHistorySheet';
import { CategoryPickerSheet } from '@/mobile/sheets/CategoryPickerSheet';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { FilterSheet } from '@/mobile/sheets/FilterSheet';
import { ImportHistorySheet } from '@/mobile/sheets/ImportHistorySheet';
import { RecordBalancesSheet } from '@/mobile/sheets/RecordBalancesSheet';
import { TransactionSheet } from '@/mobile/sheets/TransactionSheet';

type SheetKind = 'record' | 'balanceHistory' | 'importHistory' | 'deleteAccount' | 'filter';

type RecordEvent = { id: string; t: number; type: 'mark' };

/** The hero opens on a year of the balance line (the desktop's window). */
const DEFAULT_PERIOD: ChartPeriod = '1Y';

/** The menu items of `···`: 44 pt tall, body type (as Finance's menus). */
const MENU_ITEM = 'min-h-11 text-m-body';

/** The `.m-card` material. */
const CARD =
  'mb-3.5 rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] shadow-e1';

/** Route entry: another account remounts the page, so its period, filters and sheets start over. */
export default function AccountDetail() {
  const [, params] = useRoute(routes.accountDetail);
  return <AccountDetailPage key={params?.id} id={params?.id} />;
}

/**
 * The bank account detail (spec §8.4, prototype frame 04, note 8): the nav bar with the account's
 * name and `···`, its "banka · měna · číslo" line, the button pair (record a balance, import a
 * statement), the hero with the balance-record step line and the stats that have data, the flows
 * strip of 12 months (a tapped month filters the ledger), and the ledger: search, the segment, the
 * filter sheet, rows grouped by day under sticky heads, loaded page by page as it scrolls. A row
 * opens the transaction sheet on a tap or a swipe right and the category picker on a swipe left (the
 * sheet's Kategorie row opens the picker over it); a long press is its menu.
 * `?category=uncategorized` (the Finance row) opens the ledger on the payments without a category.
 */
function AccountDetailPage({ id }: { id: string | undefined }) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  // Money as text for the flows strip's accessible names and the flows line; masked while amounts
  // are hidden
  const moneyText = useMoneyText();
  const back = useBack(routes.finance);
  const search = useSearch();
  const detail = useAccountDetail(id);
  const { account, records, zones, history, historyRows, currency, historyLoaded } = detail;
  const { categories } = useCategories();

  const [state, setState] = useState<LedgerState>(() => ({
    ...DEFAULT_LEDGER,
    categoryId:
      new URLSearchParams(search).get('category') === 'uncategorized' ? 'uncategorized' : null,
  }));
  const debouncedSearch = useDebouncedValue(state.search, 300);
  const ledgerState = useMemo(
    () => ({ ...state, search: debouncedSearch }),
    [state, debouncedSearch]
  );
  const ledger = useLedger(id, ledgerState);
  const { deleteTransaction } = useLedgerMutations();
  const describeDelete = useDeleteDescription();
  const { openImport } = useImportSheet();

  const [period, setPeriod] = useState<ChartPeriod>(DEFAULT_PERIOD);
  const [sheet, setSheet] = useState<SheetKind | null>(null);
  // The row of the transaction sheet and the row of the category picker (the same one while the
  // picker is open over the sheet). They hold the row as the sheets saw it last: a write the
  // sheets make is reported through `patchTx`, since the ledger may no longer list the row once
  // it has a category (the uncategorized filter) and then cannot say what it became.
  const [openTx, setOpenTx] = useState<BankTransaction | null>(null);
  const [pickFor, setPickFor] = useState<BankTransaction | null>(null);
  const patchTx = (txId: string, patch: Partial<BankTransaction>) => {
    const apply = (tx: BankTransaction | null) => (tx?.id === txId ? { ...tx, ...patch } : tx);
    setOpenTx(apply);
    setPickFor(apply);
  };
  // "Naposledy použité" follows the account's whole history, not the filtered ledger: under
  // "Nezařazené platby" every loaded row is uncategorized and a search or segment narrows the rows
  const recentIds = useMemo(() => recentCategoryIds(history, 5), [history]);
  // The row to delete stays while its sheet slides away, so the sentence does not blank out
  const [pendingDelete, setPendingDelete] = useState<BankTransaction | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const closeSheet = (open: boolean) => !open && setSheet(null);

  const today = utcDayFloor(Date.now() / 1000);
  const money0 = (value: number) => moneyText(value, currency, { decimals: 0 });
  const signed0 = (value: number) => moneyText(value, currency, { signed: true, decimals: 0 });

  // Rows of the ledger from a flows month: tapping the selected month again lets it go
  const selectMonth = (month: number) =>
    setState((s) =>
      s.month === month
        ? { ...s, month: null, preset: 'allTime', from: null, to: null }
        : { ...s, month, preset: 'custom', ...periodForMonth(month) }
    );

  const balance = account ? parseFloat(account.balance || '0') || 0 : 0;
  const showHero = balance !== 0 || records.length > 0;
  // Until the complete history says otherwise the ledger is there (its own skeleton shows)
  const hasTransactions = !historyLoaded || history.length > 0;
  const currentMonth = monthOf(today);

  return (
    <Screen
      title={account?.name ?? ''}
      leading={<NavTextButton label={t('tabs.finance')} onClick={back} chevron />}
      trailing={
        account && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <NavIconButton aria-label={tc('labels.moreActions')}>
                <Ellipsis strokeWidth={1.75} aria-hidden />
              </NavIconButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[220px]">
              <DropdownMenuItem className={MENU_ITEM} onSelect={() => toast(t('placeholder.edit'))}>
                {t('account.menu.edit')}
              </DropdownMenuItem>
              <DropdownMenuItem className={MENU_ITEM} onSelect={() => setSheet('balanceHistory')}>
                {t('account.menu.balanceHistory')}
              </DropdownMenuItem>
              <DropdownMenuItem className={MENU_ITEM} onSelect={() => setSheet('importHistory')}>
                {t('account.menu.importHistory')}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                variant="danger"
                className={MENU_ITEM}
                onSelect={() => setSheet('deleteAccount')}
              >
                {t('account.menu.delete')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )
      }
    >
      {detail.isLoading ? (
        <DetailSkeleton />
      ) : !account ? (
        <p className="mx-0.5 mt-8 text-center text-m-body text-ink-3">{tb('detail.notFound')}</p>
      ) : (
        <>
          {/* "Fio · CZK · 2100123456/2010": the nav bar has no room for a second line */}
          <p className="mx-0.5 -mt-0.5 mb-2.5 truncate text-center text-m-caption text-ink-3">
            {[
              account.institution?.name,
              currency,
              completeAccountIdentifier(account.iban, account.bban, null),
            ]
              .filter(Boolean)
              .join(' · ')}
          </p>
          <ButtonPair
            primary={{
              label: t('account.record'),
              icon: Pencil,
              onClick: () => setSheet('record'),
            }}
            secondary={{
              label: t('account.import'),
              icon: Upload,
              onClick: () => openImport({ accountId: id, source: 'account' }),
            }}
          />
          {showHero && (
            <>
              <BalanceHero
                account={account}
                balance={balance}
                records={records}
                historyRows={historyRows}
                currency={currency}
                today={today}
                period={period}
                onPeriodChange={setPeriod}
              />
              <BalanceStats
                account={account}
                balance={balance}
                records={records}
                zones={zones}
                currency={currency}
                today={today}
              />
            </>
          )}
          {hasRecentRows(history, currentMonth) && (
            <section>
              <FlowsSection
                historyRows={historyRows}
                currentMonth={currentMonth}
                selected={state.month ?? newestMonthWithRows(historyRows)}
                onSelect={selectMonth}
                money0={money0}
                signed0={signed0}
                foreignMissing={detail.foreignMissing}
              />
            </section>
          )}
          {hasTransactions ? (
            <LedgerSection
              ledger={ledger}
              state={state}
              onStateChange={setState}
              today={today}
              categories={categories}
              onFilter={() => setSheet('filter')}
              onOpen={setOpenTx}
              onCategorize={setPickFor}
              onDelete={(tx) => {
                setPendingDelete(tx);
                setDeleteOpen(true);
              }}
            />
          ) : (
            !showHero && (
              <EmptyState
                icon={<Wallet />}
                title={tb('empty.noDataTitle')}
                description={tb('empty.noDataDescription')}
                className="px-5 py-10"
              />
            )
          )}

          <RecordBalancesSheet
            open={sheet === 'record'}
            onOpenChange={closeSheet}
            accountIds={[account.id]}
            title={t('account.record')}
          />
          <BalanceHistorySheet
            open={sheet === 'balanceHistory'}
            onOpenChange={closeSheet}
            records={records}
            currency={currency}
          />
          <ImportHistorySheet
            open={sheet === 'importHistory'}
            onOpenChange={closeSheet}
            batches={detail.batches}
          />
          {/* Deleting the account stays a placeholder in 2a: the confirmation, then the note */}
          <ConfirmSheet
            open={sheet === 'deleteAccount'}
            onOpenChange={closeSheet}
            title={tb('confirmDelete.title')}
            description={tb('confirmDelete.descriptionNamed', { name: account.name })}
            confirmLabel={tc('buttons.delete')}
            onConfirm={() => {
              toast(t('placeholder.delete'));
              setSheet(null);
            }}
          />
          <FilterSheet
            open={sheet === 'filter'}
            onOpenChange={closeSheet}
            state={state}
            categories={categories}
            onApply={setState}
          />
          <TransactionSheet
            tx={openTx}
            open={openTx !== null}
            onOpenChange={(open) => !open && setOpenTx(null)}
            category={categories.find((c) => c.id === openTx?.categoryId)}
            flow={openTx ? ledger.flowById.get(openTx.id) : undefined}
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
      )}
    </Screen>
  );
}

/**
 * "Zůstatek k 10. 10.": the balance, the 30-day flow net as the change line, and the step line
 * through the balance records with a ○ mark on each (a tap says its day and source).
 */
function BalanceHero({
  account,
  balance,
  records,
  historyRows,
  currency,
  today,
  period,
  onPeriodChange,
}: {
  account: BankAccountWithInstitution;
  balance: number;
  records: readonly BankBalanceRecord[];
  historyRows: readonly BankTransaction[];
  currency: string;
  today: number;
  period: ChartPeriod;
  onPeriodChange: (period: ChartPeriod) => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const money0 = (value: number) => moneyText(value, currency, { decimals: 0 });

  const newest = newestRecord(records);
  const asOf = newest?.asOf ?? account.balanceAsOf;
  const last30 = useMemo(
    () =>
      summarizeFlow(
        historyRows.filter((row) => row.bookingDate >= recentWindowStart(today)),
        currency
      ),
    [historyRows, today, currency]
  );
  const change = last30.count > 0 ? cashChange(balance, last30.net) : null;

  // A chart needs a line of two points; one record dated today has none yet
  const charted = useMemo(
    () => balanceLine(records, today, undefined).points.length > 1,
    [records, today]
  );
  const line = useMemo(
    () => balanceLine(records, today, chartPeriodStart(period, today)),
    [records, today, period]
  );
  const events = useMemo<RecordEvent[]>(
    () => line.marks.map((mark) => ({ id: mark.id, t: mark.t, type: 'mark' })),
    [line]
  );
  const recordById = useMemo(() => new Map(records.map((r) => [r.id, r])), [records]);
  const recordTip = (cluster: EventCluster<RecordEvent>) => {
    const record = recordById.get(cluster.events[0].id);
    if (!record) return { title: '', lines: [] };
    return {
      title: money0(Number(record.amount) || 0),
      lines: [
        tb('balanceHero.recordTip', {
          date: fmt.day(record.asOf),
          source: tb(
            record.source === 'statement'
              ? 'balanceHistory.sourceStatement'
              : 'balanceHistory.sourceManual'
          ),
        }),
      ],
    };
  };

  return (
    <Hero<RecordEvent>
      label={
        asOf != null
          ? tb('balanceHero.label', { date: fmt.day(asOf) })
          : tb('balanceHero.unknownDate')
      }
      value={balance}
      formatValue={money0}
      change={
        change
          ? {
              ratio: change.ratio,
              text: t('finance.change30', {
                amount: moneyText(change.delta, currency, { signed: true, decimals: 0 }),
              }),
            }
          : undefined
      }
      note={last30.count === 0 ? t('finance.noMovements') : undefined}
      {...(charted && {
        period,
        periods: CHART_PERIODS,
        onPeriodChange,
        points: line.points,
        step: true,
        events,
        renderEventTip: recordTip,
        chartLabel: tb('chart.balance'),
      })}
    />
  );
}

/** Změna letos, Průměrně za měsíc, Roční úrok: only the stats that have data, none without any. */
function BalanceStats({
  account,
  balance,
  records,
  zones,
  currency,
  today,
}: {
  account: BankAccountWithInstitution;
  balance: number;
  records: readonly BankBalanceRecord[];
  zones: readonly SavingsAccountZone[];
  currency: string;
  today: number;
}) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const signed0 = (value: number) => moneyText(value, currency, { signed: true, decimals: 0 });

  const thisYear = changeThisYear(records, today);
  const avg = averageMonthlyChange(records);
  const zoned = account.hasZoneDesignation && zones.length > 0;
  const simpleRate = parseFloat(account.interestRate ?? '0') || 0;
  const yearlyInterest = zoned
    ? calculateZonedInterest(balance, zones)
    : balance * (simpleRate / 100);

  const stats: StatItem[] = [];
  if (thisYear) {
    stats.push({
      label: t('account.stats.changeThisYear'),
      value: signed0(thisYear.delta),
      note: tb('balanceStats.vsRecordAt', { date: fmt.day(thisYear.baseDay) }),
    });
  }
  if (avg) {
    stats.push({
      label: t('account.stats.avgPerMonth'),
      value: signed0(avg.perMonth),
      note: avg.sinceFirst
        ? tb('balanceStats.sinceFirstRecord', { count: avg.months })
        : tb('balanceStats.last12Months'),
    });
  }
  if (zoned || simpleRate > 0) {
    stats.push({
      label: t('account.stats.yearlyInterest'),
      value: moneyText(yearlyInterest, currency, { decimals: 0 }),
      note: zoned
        ? tb('balanceStats.effectiveZoned', {
            rate: fmt.percent(balance > 0 ? yearlyInterest / balance : 0, 2),
          })
        : tb('balanceStats.simpleRate', { rate: fmt.percent(simpleRate / 100, 2) }),
    });
  }
  return stats.length > 0 ? <StatRow stats={stats} /> : null;
}

/** Toky: the 12-month strip in a card, the selected month's line under it. */
function FlowsSection({
  historyRows,
  currentMonth,
  selected,
  onSelect,
  money0,
  signed0,
  foreignMissing,
}: {
  historyRows: readonly BankTransaction[];
  currentMonth: number;
  selected: number | null;
  onSelect: (month: number) => void;
  money0: (value: number) => string;
  signed0: (value: number) => string;
  foreignMissing: readonly string[];
}) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const flows = useMemo(() => monthlyFlows(historyRows, currentMonth), [historyRows, currentMonth]);
  const flow = flows.find((f) => f.month === selected);

  return (
    <>
      <SectionHead
        title={t('account.flows.title')}
        note={
          selected !== null
            ? fmt.month(monthLabelDate(selected), { month: 'long', year: 'numeric' })
            : undefined
        }
      />
      <div className={`${CARD} px-4 pb-3 pt-3`}>
        <FlowStrip
          flows={flows}
          selected={selected}
          onSelect={onSelect}
          formatValue={money0}
          labels={{ income: tb('flows.income'), expense: tb('flows.expense') }}
          label={t('account.flows.title')}
        />
        <p className="mb-0 mt-2 text-m-caption text-ink-3 num">
          {flow && flow.count > 0
            ? tb('flows.line', {
                credits: signed0(flow.income),
                debits: signed0(-flow.expense),
                count: flow.count,
              })
            : tb('flows.noMoves')}
        </p>
        {/* Rows of a currency without a rate for their day are left out, and said so */}
        {foreignMissing.length > 0 && (
          <p className="mb-0 mt-1 text-m-caption text-loss">
            {tb('readiness.foreignMissing', { currencies: foreignMissing.join(', ') })}
          </p>
        )}
      </div>
    </>
  );
}

/** The nearest ancestor that scrolls (the screen's body): the root of the ledger's scroll observer. */
function scrollParent(element: HTMLElement): HTMLElement | null {
  for (let node = element.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node);
    if (overflowY === 'auto' || overflowY === 'scroll') return node;
  }
  return null;
}

/** Pohyby: the search, the segment and the filter button, then the rows by day as they load. */
function LedgerSection({
  ledger,
  state,
  onStateChange,
  today,
  categories,
  onFilter,
  onOpen,
  onCategorize,
  onDelete,
}: {
  ledger: Ledger;
  state: LedgerState;
  onStateChange: (update: (state: LedgerState) => LedgerState) => void;
  today: number;
  categories: ReturnType<typeof useCategories>['categories'];
  onFilter: () => void;
  onOpen: (tx: BankTransaction) => void;
  onCategorize: (tx: BankTransaction) => void;
  onDelete: (tx: BankTransaction) => void;
}) {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const filters = activeFilterCount(state);
  const categoryById = useMemo(() => new Map(categories.map((c) => [c.id, c])), [categories]);
  const groups = useMemo(() => groupByDay(ledger.rows), [ledger.rows]);

  // The next page loads when the end of the list comes within 200 px of the screen's bottom.
  // A new observer after every fetch (a page, or the refresh after a write) reports at once if the
  // end is still in view; `loadMore` itself waits out any fetch in flight.
  const [sentinel, setSentinel] = useState<HTMLDivElement | null>(null);
  const { hasMore, isFetching, isFetchingMore, loadMore } = ledger;
  useEffect(() => {
    if (!sentinel || !hasMore || isFetching) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) loadMore();
      },
      { root: scrollParent(sentinel), rootMargin: '200px' }
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [sentinel, hasMore, isFetching, loadMore]);

  const dayText = (day: number) => {
    const label = dayLabel(day, today);
    if ('key' in label) return tc(`time.${label.key}`);
    return fmt.day(label.date, {
      day: 'numeric',
      month: 'short',
      year: label.withYear ? 'numeric' : undefined,
    });
  };
  const kinds: { value: LedgerKind; label: string }[] = [
    { value: 'all', label: t('account.ledger.all') },
    { value: 'in', label: t('account.ledger.in') },
    { value: 'out', label: t('account.ledger.out') },
  ];

  return (
    <section>
      <SectionHead
        title={t('account.ledger.title')}
        note={ledger.isLoading ? undefined : t('account.ledger.count', { count: ledger.total })}
      />
      <SearchField
        value={state.search}
        onChange={(value) => onStateChange((s) => ({ ...s, search: value }))}
        placeholder={t('account.ledger.search')}
        clearLabel={tc('a11y.clearSearch')}
      />
      <div className="mb-3 flex items-center gap-2.5">
        <Segment
          value={state.kind}
          options={kinds}
          onChange={(kind) => onStateChange((s) => ({ ...s, kind }))}
          label={t('account.ledger.title')}
          className="min-w-0 flex-1"
        />
        <NavIconButton
          aria-label={
            filters > 0 ? `${t('account.ledger.filter')} (${filters})` : t('account.ledger.filter')
          }
          onClick={onFilter}
          className="relative border border-line-strong bg-paper shadow-btn [&_svg]:size-5"
        >
          <ListFilter strokeWidth={1.75} aria-hidden />
          {filters > 0 && (
            <span aria-hidden className="absolute right-2 top-2 size-1.5 rounded-full bg-dark" />
          )}
        </NavIconButton>
      </div>

      {ledger.isError ? (
        <div className="mx-0.5 py-6 text-center">
          <p className="mb-3 mt-0 text-m-body text-ink-3">{t('account.ledger.loadError')}</p>
          <Button
            variant="secondary"
            className="h-11 rounded-[10px] px-4 text-m-row"
            onClick={ledger.retry}
          >
            {t('account.ledger.retry')}
          </Button>
        </div>
      ) : ledger.isLoading ? (
        <RowsSkeleton />
      ) : groups.length === 0 ? (
        <EmptyState
          bare
          icon={<CalendarSearch />}
          title={t('account.ledger.empty')}
          description={t('account.ledger.emptyHint')}
          className="px-5 py-8"
          action={
            <Button
              variant="ghost"
              className="h-11 rounded-[10px] px-4 text-m-row"
              onClick={() => onStateChange(() => DEFAULT_LEDGER)}
            >
              {t('account.ledger.showAll')}
            </Button>
          }
        />
      ) : (
        <>
          {groups.map((group) => (
            // One block per day: its head sticks while the day's rows pass under it
            <div key={group.day}>
              <DayHead>{dayText(group.day)}</DayHead>
              <EntityRows>
                {group.rows.map((tx) => (
                  <LedgerRow
                    key={tx.id}
                    tx={tx}
                    category={tx.categoryId ? categoryById.get(tx.categoryId) : undefined}
                    flow={ledger.flowById.get(tx.id)}
                    onOpen={() => onOpen(tx)}
                    onCategorize={() => onCategorize(tx)}
                    onDelete={() => onDelete(tx)}
                  />
                ))}
              </EntityRows>
            </div>
          ))}
          {hasMore && <div ref={setSentinel} aria-hidden className="h-px" />}
          {isFetchingMore && <div aria-hidden className="skeleton mb-3.5 h-11 rounded-r4" />}
        </>
      )}
    </section>
  );
}

/** Three rows in outline while a page of the ledger loads. */
function RowsSkeleton() {
  return (
    <div
      aria-hidden
      className="mb-3.5 overflow-hidden rounded-r4 border border-line bg-paper shadow-e1"
    >
      {[0, 1, 2].map((i) => (
        <div key={i} className="flex min-h-[60px] items-center gap-3 px-3.5 py-2.5">
          <div className="skeleton size-9 flex-none rounded-full" />
          <div className="min-w-0 flex-1">
            <div className="skeleton h-3.5 w-2/5" />
            <div className="skeleton mt-2 h-3 w-3/5" />
          </div>
        </div>
      ))}
    </div>
  );
}

/** While the account loads: the button pair and the hero in outline. */
function DetailSkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 mt-6 grid grid-cols-2 gap-2.5">
        <div className="skeleton h-12 rounded-[10px]" />
        <div className="skeleton h-12 rounded-[10px]" />
      </div>
      <div className={`${CARD} px-4 pb-3.5 pt-4`}>
        <div className="skeleton h-3 w-32" />
        <div className="skeleton mt-3 h-9 w-48" />
        <div className="skeleton mt-3 h-3 w-40" />
        <div className="skeleton mt-4 h-[160px] w-full" />
      </div>
    </div>
  );
}
