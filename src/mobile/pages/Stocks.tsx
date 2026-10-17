import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Download,
  Ellipsis,
  History,
  Plus,
  RefreshCw,
  Search,
  TrendingUp,
  Upload,
} from 'lucide-react';
import { toast } from 'sonner';
import type { StockPositions } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { usePortfolioHistory } from '@/hooks/use-portfolio';
import { useWatchedStocks } from '@/hooks/use-stock-monitor';
import { amountOf, totalOf, type StockRow } from '@/utils/positions';
import type { EventCluster } from '@/utils/chart-scale';
import { CHART_PERIODS, chartPeriodStart, type ChartPeriod } from '@/utils/period';
import {
  firstTradeDay,
  tickerSummary,
  tradeDayEvents,
  type TradeDayEvent,
} from '@/utils/trade-events';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { EmptyState } from '@/components/common/EmptyState';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';
import { AddInvestmentModal } from '@/components/stocks/AddInvestmentModal';
import { EntityRows } from '@/mobile/components/EntityRow';
import { Hero } from '@/mobile/components/Hero';
import { Money } from '@/mobile/components/Money';
import { SearchField } from '@/mobile/components/SearchField';
import { Segment } from '@/mobile/components/Segment';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { MoreRow } from '@/mobile/components/wealth/MoreRow';
import { PositionRow } from '@/mobile/components/wealth/PositionRow';
import { SortPill } from '@/mobile/components/wealth/SortPill';
import { WatchedRow } from '@/mobile/components/wealth/WatchedRow';
import { useBack } from '@/mobile/hooks/use-back';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import {
  useRefreshStockPrices,
  useStockPositions,
  useStockTrades,
  useTwrThisYear,
} from '@/mobile/hooks/use-stocks';
import { changeSinceDayOptions } from '@/mobile/lib/change';
import { classTrendPoints, pointsChange, trendChangeBase } from '@/mobile/lib/class-trend';
import { shortDayOptions } from '@/mobile/lib/format-day';
import { foldRows, positionRows, priceStamp, type PositionSort } from '@/mobile/lib/position-rows';
import { statMoneyOptions } from '@/mobile/lib/stat-money';
import { NavIconButton, NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** Whole amounts: the class page shows no decimals. */
const WHOLE = { decimals: 0 } as const;
/** The classes of a menu item on the phone: 44 pt tall, body type, 18 pt icons. */
const MENU_ITEM = 'min-h-11 text-m-body [&_svg]:size-[18px]';

type Tab = 'positions' | 'watched';

/** The tone of a signed result: green above zero, red under it, none at zero or unknown. */
const toneOf = (value: number | null): StatItem['tone'] =>
  value === null || value === 0 ? undefined : value > 0 ? 'gain' : 'loss';

/**
 * Akcie, the class page of Majetek (spec §8.8, prototype frame 11): the portfolio value with its
 * line and the buy and sell days, three stats (the unrealized return, the dividends of a year, the
 * return of this year), then the segment Pozice · Sledované. Positions are rows with sort, search
 * and the first eight until "Zobrazit všech N"; the watched ones are rows with their own price.
 * The numbers are the desktop's (the same queries), in the report currency; every amount goes
 * through the eye of Přehled.
 */
export default function Stocks() {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const back = useBack(routes.wealth);
  const { positions, rows, reportCurrency, isLoading } = useStockPositions();
  const refresh = useRefreshStockPrices();
  const { data: watched = [] } = useWatchedStocks();
  const [tab, setTab] = useState<Tab>('positions');
  const [showClosed, setShowClosed] = useState(false);
  const [addOpen, setAddOpen] = useState(false);

  const open = rows.filter((row) => row.quantity > 0);
  const later = () => toast(t('wealth.placeholder.later'));

  // The newest price of the list: "ceny 9:40" today, "ceny 12. 10." on an older day
  const fetchedAt = rows.reduce<number | null>(
    (newest, row) =>
      row.fetchedAt !== null && (newest === null || row.fetchedAt > newest)
        ? row.fetchedAt
        : newest,
    null
  );
  const stamp = priceStamp(fetchedAt, Date.now() / 1000);
  const subLine = [
    ts('summary.positions', { count: open.length }),
    stamp.kind === 'today'
      ? t('wealth.stocks.pricesAt', { time: fmt.time(stamp.at) })
      : stamp.kind === 'day'
        ? t('wealth.stocks.pricesOn', { date: fmt.day(stamp.at, shortDayOptions(stamp.at)) })
        : ts('summary.notUpdated'),
  ].join(' · ');

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
              <DropdownMenuItem
                className={MENU_ITEM}
                disabled={refresh.isPending}
                onSelect={() => refresh.mutate()}
              >
                <RefreshCw strokeWidth={1.75} aria-hidden />
                {ts('refreshPrices')}
              </DropdownMenuItem>
              <DropdownMenuItem className={MENU_ITEM} onSelect={later}>
                <Upload strokeWidth={1.75} aria-hidden />
                {t('wealth.stocks.menu.import')}
              </DropdownMenuItem>
              <DropdownMenuItem className={MENU_ITEM} onSelect={later}>
                <History strokeWidth={1.75} aria-hidden />
                {ts('importHistory.title')}
              </DropdownMenuItem>
              <DropdownMenuCheckboxItem
                className="min-h-11 text-m-body"
                checked={showClosed}
                onCheckedChange={setShowClosed}
              >
                {t('wealth.stocks.menu.closed')}
              </DropdownMenuCheckboxItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem className={MENU_ITEM} onSelect={later}>
                <Download strokeWidth={1.75} aria-hidden />
                {tc('buttons.export')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <NavIconButton aria-label={ts('addInvestment')} onClick={() => setAddOpen(true)}>
            <Plus strokeWidth={1.75} aria-hidden />
          </NavIconButton>
        </>
      }
    >
      {isLoading ? (
        <StocksSkeleton />
      ) : rows.length === 0 ? (
        <EmptyState
          icon={<TrendingUp />}
          title={ts('empty.title')}
          description={ts('empty.description')}
          className="px-5 py-10"
          action={
            <Button
              className="h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-3 text-m-row"
              onClick={() => setAddOpen(true)}
            >
              <Plus aria-hidden />
              {ts('addInvestment')}
            </Button>
          }
        />
      ) : (
        <>
          {/* The nav bar keeps its one title, so the count and the age of the prices are the first line */}
          <p className="mx-0.5 -mt-0.5 mb-2.5 truncate text-center text-m-caption text-ink-3">
            {subLine}
          </p>
          {/* Amounts the backend could not convert into the report currency (left out) */}
          <ConversionQualityNote
            quality={positions?.fxQuality}
            currency={reportCurrency}
            className="mx-0.5 mb-3.5"
          />
          <PortfolioHero
            totalValue={totalOf(positions?.totals.value)}
            invested={totalOf(positions?.totals.costBasis)}
            currency={reportCurrency}
          />
          <PortfolioStats positions={positions} currency={reportCurrency} />
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
            <PositionsTab
              rows={rows}
              currency={reportCurrency}
              showClosed={showClosed}
              dimmed={refresh.isPending}
            />
          </div>
          {tab === 'watched' &&
            (watched.length === 0 ? (
              <p className="mx-0.5 py-6 text-center text-m-body text-ink-3">
                {t('wealth.stocks.watchedEmpty')}
              </p>
            ) : (
              <EntityRows>
                {watched.map((row) => (
                  <WatchedRow key={row.id} row={row} />
                ))}
              </EntityRows>
            ))}
        </>
      )}
      <AddInvestmentModal open={addOpen} onOpenChange={setAddOpen} />
    </Screen>
  );
}

/**
 * The portfolio value (spec §8.8): the line of the period with today's value as its last point
 * and the buy and sell days as marks (a tap opens the day), the change over the period and what
 * is invested. The legend names the marks.
 */
function PortfolioHero({
  totalValue,
  invested,
  currency,
}: {
  totalValue: number | null;
  invested: number | null;
  currency: string;
}) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const { t: td } = useTranslation('dashboard');
  const fmt = useFormat();
  const moneyText = useMoneyText();
  const { transactions, convertAt } = useStockTrades();
  const [period, setPeriod] = useState<ChartPeriod>('30D');

  // No horizon starts before the first trade; the start is a whole UTC day, so the query holds still
  const earliest = useMemo(() => firstTradeDay(transactions), [transactions]);
  const start = chartPeriodStart(period, Date.now() / 1000, earliest);
  const { data: history } = usePortfolioHistory(start);

  const points = useMemo(
    () => classTrendPoints(history, 'investments', currency, totalValue, Date.now() / 1000),
    [history, currency, totalValue]
  );
  const change = pointsChange(points);
  // "over 30 days", or "vs 3. 10." when the line starts later than the period (a young portfolio):
  // the period's own start, not the one clamped to the first trade
  const changeBase = trendChangeBase(points, chartPeriodStart(period, Date.now() / 1000));
  const changeLabel =
    changeBase === null
      ? t(`overview.change.${period}`)
      : td('changeSince', { date: fmt.day(changeBase, changeSinceDayOptions(changeBase)) });

  // Every trade at its own day's rate, like the history line it sits on; events before the horizon
  // would pile up on the left edge
  const events = useMemo(
    () =>
      tradeDayEvents(transactions, (amount, from, date) =>
        convertAt(amount, from, currency, date)
      ).filter((event) => start === undefined || event.t >= start),
    [transactions, convertAt, currency, start]
  );
  const eventTip = (cluster: EventCluster<TradeDayEvent>) => {
    if (cluster.events.length > 1) {
      const first = cluster.events[0].t;
      const last = cluster.events[cluster.events.length - 1].t;
      return {
        title: ts('chart.events.cluster', { count: cluster.events.length }),
        lines: [`${fmt.day(first)} – ${fmt.day(last)}`],
      };
    }
    const event = cluster.events[0];
    return {
      title: ts(event.type === 'sell' ? 'chart.events.sell' : 'chart.events.buy', {
        tickers: tickerSummary(event.tickers),
      }),
      lines: [
        event.amount === null
          ? fmt.day(event.t)
          : `${fmt.day(event.t)} · ${moneyText(event.amount, currency, WHOLE)}`,
      ],
    };
  };

  return (
    <Hero
      label={ts('summary.totalValue')}
      value={totalValue}
      formatValue={(value) => moneyText(value, currency, WHOLE)}
      change={
        change
          ? {
              ratio: change.ratio ?? 0,
              text: (
                <>
                  <Money
                    value={change.delta}
                    currency={currency}
                    options={{ signed: true, ...WHOLE }}
                  />{' '}
                  {changeLabel}
                  {invested !== null && (
                    <>
                      {' · '}
                      {t('wealth.stocks.invested', {
                        amount: moneyText(invested, currency, WHOLE),
                      })}
                    </>
                  )}
                </>
              ),
            }
          : undefined
      }
      note={t('overview.stats.noHistory')}
      period={period}
      periods={CHART_PERIODS}
      onPeriodChange={setPeriod}
      points={points}
      zeroBaseline={period === 'All'}
      events={events}
      renderEventTip={eventTip}
      chartLabel={ts('summary.totalValue')}
    >
      {points.length > 1 && (
        <ChartLegend
          items={[
            { label: ts('chart.legend.value'), swatch: { kind: 'line' } },
            { label: ts('chart.legend.buy'), swatch: { kind: 'event', type: 'buy' } },
            { label: ts('chart.legend.sell'), swatch: { kind: 'event', type: 'sell' } },
          ]}
        />
      )}
    </Hero>
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
      tone: toneOf(unrealized),
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
      tone: toneOf(twr),
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

/**
 * Pozice: a toolbar (the sort pill and the search toggle), the search field while it is on, then
 * the positions as rows — the first eight, "Zobrazit všech N" for the rest. Closed positions
 * wait behind the `···` menu. A search that finds nothing says so instead of showing an empty list.
 */
function PositionsTab({
  rows,
  currency,
  showClosed,
  dimmed,
}: {
  rows: StockRow[];
  currency: string;
  showClosed: boolean;
  dimmed: boolean;
}) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const [sort, setSort] = useState<PositionSort>('value');
  const [search, setSearch] = useState('');
  const [searchOpen, setSearchOpen] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const searchBox = useRef<HTMLDivElement>(null);

  // A search that is out of sight must not keep filtering the list
  const toggleSearch = () => {
    if (searchOpen) setSearch('');
    setSearchOpen(!searchOpen);
  };

  // Opening the search is for typing: the field takes the focus (and the keyboard) at once
  useEffect(() => {
    if (searchOpen) searchBox.current?.querySelector('input')?.focus();
  }, [searchOpen]);

  const shownRows = positionRows(rows, { sort, search, showClosed });
  const { shown, hidden } = foldRows(shownRows, expanded);

  return (
    <>
      <div className="mb-3 flex items-center justify-between gap-2.5">
        <SortPill
          value={sort}
          options={[
            { value: 'value', label: t('wealth.stocks.sort.value') },
            { value: 'gain', label: t('wealth.stocks.sort.gain') },
            { value: 'name', label: t('wealth.stocks.sort.name') },
          ]}
          onChange={setSort}
          label={t('wealth.stocks.sort.label')}
        />
        <NavIconButton
          aria-label={t('wealth.stocks.searchToggle')}
          aria-pressed={searchOpen}
          onClick={toggleSearch}
          className="border border-line-strong bg-paper shadow-btn [&_svg]:size-5"
        >
          <Search strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      </div>
      {searchOpen && (
        <div ref={searchBox}>
          <SearchField
            value={search}
            onChange={setSearch}
            placeholder={ts('table.search')}
            clearLabel={t('wealth.stocks.clearSearch')}
          />
        </div>
      )}
      {shownRows.length === 0 ? (
        <p className="mx-0.5 py-6 text-center text-m-body text-ink-3">
          {search.trim() === '' ? ts('table.onlyClosedPositions') : ts('table.noResults.title')}
        </p>
      ) : (
        <EntityRows className={dimmed ? 'opacity-50 transition-opacity' : undefined}>
          {shown.map((row) => (
            <PositionRow key={row.id} row={row} currency={currency} />
          ))}
          {hidden > 0 && (
            <MoreRow
              label={t('wealth.stocks.showAll', { count: shownRows.length })}
              onClick={() => setExpanded(true)}
            />
          )}
        </EntityRows>
      )}
    </>
  );
}

/** While the positions load: the hero, the stats and a few rows in outline. */
function StocksSkeleton() {
  return (
    <div aria-hidden>
      <div className="mb-3.5 rounded-r4 border border-line bg-paper px-4 pb-3 pt-4 shadow-e1">
        <div className="skeleton h-3 w-24" />
        <div className="skeleton mt-3 h-9 w-56" />
        <div className="skeleton mt-3 h-3 w-40" />
        <div className="skeleton mt-4 h-[160px] w-full" />
      </div>
      <div className="mb-3.5 grid grid-cols-3 gap-2.5">
        {[0, 1, 2].map((i) => (
          <div key={i} className="rounded-r3 border border-line bg-paper px-3 py-2.5 shadow-e1">
            <div className="skeleton h-2.5 w-3/5" />
            <div className="skeleton mt-2.5 h-4 w-4/5" />
          </div>
        ))}
      </div>
      <div className="mb-3.5 overflow-hidden rounded-r4 border border-line bg-paper shadow-e1">
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
    </div>
  );
}
