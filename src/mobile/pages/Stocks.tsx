import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import {
  Archive,
  Download,
  Ellipsis,
  History,
  Plus,
  RefreshCw,
  TrendingUp,
  Upload,
} from 'lucide-react';
import { toast } from 'sonner';
import type { StockImportBatch, StockPositions, WatchedStockRow } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { useWatchedStocks } from '@/hooks/use-stock-monitor';
import { useStockImportMutations } from '@/hooks/use-stock-import-mutations';
import { amountOf, totalOf } from '@/utils/positions';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { ScaledConversionNote } from '@/mobile/components/Scaled';
import { AddInvestmentModal } from '@/components/stocks/AddInvestmentModal';
import { StockImportHistoryDialog } from '@/components/stocks/StockImportHistoryDialog';
import { StockImportDialog } from '@/components/stocks/import/StockImportDialog';
import { useStockImportBatches } from '@/components/stocks/import/use-stock-import-data';
import { RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SearchField } from '@/mobile/components/SearchField';
import { Segment } from '@/mobile/components/Segment';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { ClassEmpty } from '@/mobile/components/wealth/ClassEmpty';
import { ClassHero } from '@/mobile/components/wealth/ClassHero';
import { ClassSkeleton } from '@/mobile/components/wealth/ClassSkeleton';
import { PositionList } from '@/mobile/components/wealth/PositionList';
import { PricesLine } from '@/mobile/components/wealth/PricesLine';
import { TickerResultRow } from '@/mobile/components/wealth/TickerResultRow';
import { WatchedRow } from '@/mobile/components/wealth/WatchedRow';
import { useBack } from '@/mobile/hooks/use-back';
import { useClassExport } from '@/mobile/hooks/use-class-export';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useRemembered } from '@/mobile/hooks/use-remembered';
import {
  useRefreshStockPrices,
  useStockPositions,
  useStockTrades,
  useTwrThisYear,
} from '@/mobile/hooks/use-stocks';
import { useTickerSearch } from '@/mobile/hooks/use-ticker-search';
import { stocksView, type StocksTab } from '@/mobile/lib/position-rows';
import { statMoneyOptions, statTone } from '@/mobile/lib/stat-money';
import { ClosedPositionsSheet } from '@/mobile/sheets/ClosedPositionsSheet';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { NavIconButton, NavTextButton } from '@/mobile/shell/NavBar';
import { positionPath, routes, watchPath } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** Whole amounts: the class page shows no decimals. */
const WHOLE = { decimals: 0 } as const;

/**
 * Akcie, the class page of Majetek (spec §8.8, prototype frame 11): the portfolio value with its
 * line and the buy and sell days, three stats (the unrealized return, the dividends of a year, the
 * return of this year), then the segment Pozice · Sledované. Positions are rows with sort, search
 * and the first eight until "Zobrazit všech N"; the watched ones are rows with their own price.
 * Someone who only watches has no positions to show a value for: the segment comes first, opened
 * on Sledované, with the empty state inside Pozice. A class with nothing in it (no positions, nothing
 * watched) shows the same segment — without the hero, the stats and the prices line — opened on
 * Pozice, whose empty state adds the first position, while Sledované holds the search that starts
 * a watch: the search is the only way to a watch detail. The numbers are the desktop's (the same
 * queries), in the report currency; every amount goes through the eye of Přehled. The list holds
 * the open positions; the closed ones are the sheet Uzavřené pozice in the `···` menu, and Export
 * there hands the transactions' CSV to the Files picker. Importovat CSV opens the desktop's broker
 * import wizard as it is (the same file picker, broker detection, review and split decisions), and
 * Historie importů, shown once there is one, lists the recorded imports with the desktop's delete;
 * its confirmation is the phone's sheet with the desktop's sentences. The sort, the period and the
 * segment are remembered for the session, so a visit to a position does not reset them.
 */
export default function Stocks() {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const back = useBack(routes.wealth);
  const { positions, rows, reportCurrency, isLoading, isError } = useStockPositions();
  const { transactions: trades, convertAt } = useStockTrades();
  const refresh = useRefreshStockPrices();
  const { exportCsv, isPending: exporting } = useClassExport('stocks');
  const { data: watched = [], isLoading: watchedLoading } = useWatchedStocks();
  // How the page was left is kept for the session (a position remounts it); the add dialog and the
  // closed sheet are not, so they do not reopen on return. `null`: no segment chosen yet, so the
  // view's own opens.
  const [chosenTab, setTab] = useRemembered<StocksTab | null>('stocks.tab', null);
  const [addOpen, setAddOpen] = useState(false);
  const [closedOpen, setClosedOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [importsOpen, setImportsOpen] = useState(false);
  // The recorded imports (CSV and MCP) for Historie importů; the item shows once there is one
  const { data: importBatches = [] } = useStockImportBatches(true);
  const { undoBatch } = useStockImportMutations();
  const [pendingBatch, setPendingBatch] = useState<StockImportBatch | null>(null);
  const confirmDeleteBatch = async () => {
    if (!pendingBatch) return;
    try {
      const outcome = await undoBatch.mutateAsync(pendingBatch.id);
      toast(
        pendingBatch.remainingCount > 0
          ? ts('importHistory.deleted', { count: outcome.removed })
          : ts('importHistory.recordDeleted')
      );
      setPendingBatch(null);
    } catch (error) {
      toast.error(ts('importWizard.toast.undoFailed'), { description: String(error) });
    }
  };
  const view = stocksView(rows, watched, isError);
  const tab = chosenTab ?? view.defaultTab;
  const hasPositions = rows.length > 0;

  const open = rows.filter((row) => row.quantity > 0);

  return (
    <Screen
      title={ts('title')}
      leading={<NavTextButton label={t('tabs.wealth')} onClick={back} chevron />}
      trailing={
        <>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <NavIconButton aria-label={tc('labels.moreActions')}>
                <Ellipsis strokeWidth={1.75} aria-hidden />
              </NavIconButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[240px]">
              <RowMenuItem disabled={refresh.isPending} onSelect={() => refresh.mutate()}>
                <RefreshCw strokeWidth={1.75} aria-hidden />
                {ts('refreshPrices')}
              </RowMenuItem>
              <RowMenuItem onSelect={() => setClosedOpen(true)}>
                <Archive strokeWidth={1.75} aria-hidden />
                {t('wealth.stocks.menu.closed')}
              </RowMenuItem>
              <DropdownMenuSeparator />
              <RowMenuItem onSelect={() => setImportOpen(true)}>
                <Upload strokeWidth={1.75} aria-hidden />
                {ts('importCSV')}
              </RowMenuItem>
              {importBatches.length > 0 && (
                <RowMenuItem onSelect={() => setImportsOpen(true)}>
                  <History strokeWidth={1.75} aria-hidden />
                  {ts('importHistory.title')}
                </RowMenuItem>
              )}
              <RowMenuItem disabled={exporting} onSelect={() => void exportCsv()}>
                <Download strokeWidth={1.75} aria-hidden />
                {tc('buttons.export')}
              </RowMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <NavIconButton aria-label={ts('addInvestment')} onClick={() => setAddOpen(true)}>
            <Plus strokeWidth={1.75} aria-hidden />
          </NavIconButton>
        </>
      }
    >
      {isLoading || (!hasPositions && watchedLoading) ? (
        <ClassSkeleton />
      ) : view.kind === 'error' ? (
        <LoadFailed />
      ) : (
        <>
          {hasPositions && (
            <>
              {/* The nav bar keeps its one title, so the count and the age of the prices are the first line */}
              <PricesLine count={ts('summary.positions', { count: open.length })} rows={rows} />
              {/* Amounts the backend could not convert into the report currency (left out) */}
              <ScaledConversionNote
                quality={positions?.fxQuality}
                currency={reportCurrency}
                className="mx-0.5 mb-3.5"
              />
              <ClassHero
                trendClass="investments"
                label={ts('summary.totalValue')}
                totalValue={totalOf(positions?.totals.value)}
                invested={totalOf(positions?.totals.costBasis)}
                currency={reportCurrency}
                trades={trades}
                convertAt={convertAt}
                rememberKey="stocks"
              />
              <PortfolioStats positions={positions} currency={reportCurrency} />
            </>
          )}
          <Segment
            value={tab}
            options={[
              {
                value: 'positions',
                label: t('wealth.stocks.positionsTab', { count: open.length }),
              },
              { value: 'watched', label: t('wealth.stocks.watchedTab', { count: watched.length }) },
            ]}
            onChange={setTab}
            label={ts('title')}
            className="mb-2"
          />
          {/* Kept mounted while the other segment shows, so its sort, search and fold stay */}
          <div hidden={tab !== 'positions'}>
            {hasPositions ? (
              <PositionList
                rows={rows}
                hrefOf={(row) => positionPath(row.id)}
                currency={reportCurrency}
                dimmed={refresh.isPending}
                rememberKey="stocks"
              />
            ) : isError ? (
              <LoadFailed />
            ) : (
              <NoPositions onAdd={() => setAddOpen(true)} />
            )}
          </div>
          {tab === 'watched' && <WatchedList rows={watched} />}
        </>
      )}
      <AddInvestmentModal open={addOpen} onOpenChange={setAddOpen} />
      <ClosedPositionsSheet
        open={closedOpen}
        onOpenChange={setClosedOpen}
        rows={rows}
        trades={trades}
        currency={reportCurrency}
        hrefOf={(row) => positionPath(row.id)}
      />
      {/* "Zobrazit akcie" lands here; the phone has no stock analysis to assign tags in */}
      <StockImportDialog
        open={importOpen}
        onOpenChange={setImportOpen}
        stocksPath={routes.stocks}
        analysisPath={null}
      />
      <StockImportHistoryDialog
        open={importsOpen}
        onOpenChange={setImportsOpen}
        batches={importBatches}
        onDelete={(batch) => {
          setImportsOpen(false);
          setPendingBatch(batch);
        }}
      />
      <ConfirmSheet
        open={pendingBatch !== null}
        onOpenChange={(next) => {
          if (!next) setPendingBatch(null);
        }}
        title={ts('importHistory.deleteTitle')}
        description={
          pendingBatch && pendingBatch.remainingCount > 0
            ? `${ts('importWizard.undo.description', {
                count: pendingBatch.remainingCount,
                file: pendingBatch.fileName,
              })} ${ts('importWizard.undo.consequences')}`
            : ts('importHistory.recordDescription', { file: pendingBatch?.fileName ?? '' })
        }
        confirmLabel={ts(
          pendingBatch && pendingBatch.remainingCount > 0
            ? 'importHistory.deleteConfirm'
            : 'importHistory.deleteRecord'
        )}
        isPending={undoBatch.isPending}
        onConfirm={() => void confirmDeleteBatch()}
      />
    </Screen>
  );
}

/** The ticker search starts at two characters, as the desktop's does. */
const SEARCH_FROM = 2;

/**
 * The Sledované segment (spec §8.8): the search field (Yahoo) on top, under it the watched stocks
 * as rows — or, from two characters on, what the search found. A hit opens the watch detail of its
 * symbol and clears the field, so coming back shows the list; following happens on the detail, as
 * on the desktop. While an answer is on its way the last hits stay (dimmed), and "no results" is
 * only said for a term that has been asked.
 */
function WatchedList({ rows }: { rows: WatchedStockRow[] }) {
  const { t } = useTranslation('mobile');
  const { t: tm } = useTranslation('stockMonitor');
  const [, setLocation] = useLocation();
  const [query, setQuery] = useState('');
  const { results, isFetching } = useTickerSearch(query);
  const searching = query.trim().length >= SEARCH_FROM;

  const open = (symbol: string) => {
    setQuery('');
    setLocation(watchPath(symbol));
  };

  return (
    <>
      <SearchField
        value={query}
        onChange={setQuery}
        placeholder={tm('search.placeholder')}
        clearLabel={t('wealth.stocks.clearSearch')}
      />
      {searching ? (
        results.length > 0 ? (
          <EntityRows className={cn('transition-opacity', isFetching && 'opacity-60')}>
            {results.map((result) => (
              <TickerResultRow key={result.symbol} result={result} onPick={open} />
            ))}
          </EntityRows>
        ) : (
          <p role="status" className="mx-0.5 py-6 text-center text-m-body text-ink-3">
            {isFetching ? tm('search.searching') : tm('search.noResults')}
          </p>
        )
      ) : rows.length === 0 ? (
        <p className="mx-0.5 py-6 text-center text-m-body text-ink-3">
          {t('wealth.stocks.watchedEmpty')}
        </p>
      ) : (
        <EntityRows>
          {rows.map((row) => (
            <WatchedRow key={row.id} row={row} />
          ))}
        </EntityRows>
      )}
    </>
  );
}

/**
 * Three stats (spec §8.8): the unrealized return with the realized one beside it, the dividends
 * of a year with their yield, and the return of this year — the time-weighted one that the buys
 * and sells cannot distort, with the money-weighted one in its note.
 */
function PortfolioStats({
  positions,
  currency,
}: {
  positions: StockPositions | undefined;
  currency: string;
}) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { twr, mwr, isLoading: twrLoading } = useTwrThisYear(positions !== undefined);

  const totals = positions?.totals;
  const value = totalOf(totals?.value);
  const unrealized = totalOf(totals?.unrealizedGain);
  const realized = amountOf(totals?.realizedGain);
  const dividends = totalOf(totals?.yearlyDividend);

  const statMoney = (amount: number, signed = false) =>
    moneyText(amount, currency, { ...statMoneyOptions(amount), signed });

  const stats: StatItem[] = [
    {
      label: ts('detail.unrealized'),
      value: unrealized === null ? '—' : statMoney(unrealized, true),
      tone: statTone(unrealized),
      note: [
        unrealized !== null &&
          totals !== undefined &&
          fmt.percent(totals.unrealizedGainPercent / 100, 1, { signed: true }),
        realized !== null &&
          realized !== 0 &&
          ts('summary.realizedNote', {
            amount: moneyText(realized, currency, { signed: true, ...WHOLE }),
          }),
      ]
        .filter(Boolean)
        .join(' · '),
    },
    {
      label: t('wealth.stocks.dividendsYear'),
      value: dividends ? statMoney(dividends) : '—',
      note:
        dividends && value !== null && value > 0
          ? t('wealth.stocks.yieldNote', { percent: fmt.percent(dividends / value, 1) })
          : undefined,
    },
    {
      label: t('wealth.stocks.twrYtd'),
      value: twr === null ? '—' : fmt.percent(twr / 100, 1, { signed: true }),
      tone: statTone(twr),
      // While the report loads there is nothing to say yet; "no history yet" is for a report that
      // has fewer than two days (or failed)
      note:
        twr !== null
          ? t('wealth.stocks.twrNote', {
              mwr: fmt.percent((mwr ?? 0) / 100, 1, { signed: true }),
            })
          : twrLoading
            ? undefined
            : t('wealth.stocks.twrNoHistory'),
    },
  ];
  return <StatRow stats={stats} />;
}

/** Nothing owned: the empty state of the class, with the button that adds the first position. */
function NoPositions({ onAdd }: { onAdd: () => void }) {
  const { t: ts } = useTranslation('stocks');
  return (
    <ClassEmpty
      icon={<TrendingUp />}
      title={ts('empty.title')}
      description={ts('empty.description')}
      addLabel={ts('addInvestment')}
      onAdd={onAdd}
    />
  );
}

/** The positions could not be loaded: said as such, not shown as "no positions yet". */
function LoadFailed() {
  const { t } = useTranslation('mobile');
  return (
    <p role="alert" className="mx-0.5 py-6 text-center text-m-body text-ink-3">
      {t('wealth.stocks.loadFailed')}
    </p>
  );
}
