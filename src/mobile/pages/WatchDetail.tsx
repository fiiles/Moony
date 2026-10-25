import { useMemo, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { useRoute } from 'wouter';
import { Ellipsis, Eye, EyeOff, Plus, RefreshCw, Target } from 'lucide-react';
import type { StockMonitorDetail, StockPricePoint } from '@shared/schema';
import {
  useStockMonitorDetail,
  useStockPriceRange,
  useWatchedStocks,
} from '@/hooks/use-stock-monitor';
import { useStockMonitorMutations } from '@/hooks/use-stock-monitor-mutations';
import { useFormat } from '@/lib/use-format';
import { exchangeName } from '@/utils/exchange-names';
import { CHART_PERIODS, chartPeriodStart, type ChartPeriod } from '@/utils/period';
import { changeFromReference, dayChange, targetDistance } from '@/utils/stock-monitor';
import { ScaledChartLegend, ScaledEmptyState } from '@/mobile/components/Scaled';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { AddInvestmentModal } from '@/components/stocks/AddInvestmentModal';
import { ButtonPair } from '@/mobile/components/ButtonPair';
import { RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { Hero } from '@/mobile/components/Hero';
import { StatRow, type StatItem } from '@/mobile/components/StatRow';
import { ClassSkeleton } from '@/mobile/components/wealth/ClassSkeleton';
import { WatchKeyData } from '@/mobile/components/wealth/WatchKeyData';
import { WatchNotes } from '@/mobile/components/wealth/WatchNotes';
import { useBack } from '@/mobile/hooks/use-back';
import { leaveAfterSheet } from '@/mobile/hooks/use-leave-after-sheet';
import { useRemembered } from '@/mobile/hooks/use-remembered';
import { changeSinceDayOptions } from '@/mobile/lib/change';
import { trendChangeBase } from '@/mobile/lib/class-trend';
import { shortDayOptions } from '@/mobile/lib/format-day';
import { statTone } from '@/mobile/lib/stat-money';
import {
  priceOf,
  shownPercent,
  watchPoints,
  watchPriceAt,
  watchPriceText,
  watchRange,
  watchSincePeriod,
  watchTargetState,
} from '@/mobile/lib/watch-detail';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { WatchTargetSheet } from '@/mobile/sheets/WatchTargetSheet';
import { NavIconButton, NavTextButton } from '@/mobile/shell/NavBar';
import { routes } from '@/mobile/shell/routes';
import { Screen } from '@/mobile/shell/Screen';

/** A series that is not there yet: one array, so a memo over it does not run on every render. */
const NO_POINTS: StockPricePoint[] = [];

/** The ticker of the path: escaped by `watchPath`, and never a reason to crash on a path that is not. */
function tickerOf(param: string | undefined): string {
  if (!param) return '';
  try {
    return decodeURIComponent(param);
  } catch {
    return param;
  }
}

/**
 * A watched ticker, followed or not (spec §8.8, ruling 8: a search result opens it): the ticker in
 * the nav bar with the name and the exchange under it, the price in the stock's own currency with
 * its line over the phone's five periods and the target as the hero's dashed line, then Nastavit
 * cíl (or Sledovat for a ticker that is not followed yet) beside Koupit, three stats — today, the
 * target, since the stock was followed — the notes and the key data. Every price here is Yahoo's
 * public number, so it stays visible while amounts are hidden; the numbers are the desktop
 * detail's (the same queries and helpers). A ticker Yahoo knows nothing about says so instead of
 * showing a blank page, and a followed one can still be unfollowed from `···`.
 */
export default function WatchDetail() {
  const { t } = useTranslation('mobile');
  const { t: tm } = useTranslation('stockMonitor');
  const { t: ts } = useTranslation('stocks');
  const { t: td } = useTranslation('dashboard');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const [, params] = useRoute(routes.watchDetail);
  const ticker = tickerOf(params?.ticker);
  const back = useBack(routes.stocks);
  const [period, setPeriod] = useRemembered<ChartPeriod>('watch.period', '30D');
  const [targetOpen, setTargetOpen] = useState(false);
  const [buyOpen, setBuyOpen] = useState(false);
  const [confirmUnfollow, setConfirmUnfollow] = useState(false);
  // One clock for the page: the range and its cut do not move while it is open
  const [nowSec] = useState(() => Math.floor(Date.now() / 1000));

  const { data: detail, isLoading, isError } = useStockMonitorDetail(ticker || undefined);
  const { data: watched } = useWatchedStocks();
  const { followMutation, unfollowMutation, notesMutation, refreshDetailMutation } =
    useStockMonitorMutations();

  const { range, from } = useMemo(() => watchRange(period, nowSec), [period, nowSec]);
  const { data: series } = useStockPriceRange(ticker || undefined, range);
  const followedAt = detail?.followedAt ?? null;
  // The second range of the desktop detail: the shortest that holds the day it was followed
  const { data: sinceSeries } = useStockPriceRange(
    followedAt !== null ? ticker : undefined,
    followedAt !== null ? watchSincePeriod(followedAt, nowSec) : range
  );

  const currency = detail?.currency ?? null;
  const current = priceOf(detail?.currentPrice ?? null);
  const targetValue = priceOf(detail?.targetPrice ?? null);
  const fetchedAt = detail?.priceFetchedAt ?? null;
  const points = useMemo(
    () => watchPoints(series ?? NO_POINTS, from, current, fetchedAt, targetValue, nowSec),
    [series, from, current, fetchedAt, targetValue, nowSec]
  );

  // A ticker whose detail cannot be read may still be on the watchlist: it must be removable
  const followed =
    detail?.followed ??
    watched?.some((row) => row.ticker.toUpperCase() === ticker.toUpperCase()) ??
    false;
  const name = detail?.longName ?? detail?.shortName ?? detail?.ticker ?? ticker;
  const detailTicker = detail?.ticker;
  // The dialog resets its form whenever it is handed another object, so this one is rebuilt only
  // when what it holds changes — not on every render of the page
  const buyInitial = useMemo(
    () => (detailTicker ? { ticker: detailTicker, companyName: name, currency } : undefined),
    [detailTicker, name, currency]
  );

  const menu = ticker !== '' && (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <NavIconButton aria-label={tc('labels.moreActions')}>
          <Ellipsis strokeWidth={1.75} aria-hidden />
        </NavIconButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[240px]">
        <RowMenuItem
          disabled={refreshDetailMutation.isPending}
          onSelect={() => refreshDetailMutation.mutate(ticker)}
        >
          <RefreshCw strokeWidth={1.75} aria-hidden />
          {tm('actions.refreshShort')}
        </RowMenuItem>
        {followed && (
          <>
            <DropdownMenuSeparator />
            <RowMenuItem variant="danger" onSelect={() => setConfirmUnfollow(true)}>
              <EyeOff strokeWidth={1.75} aria-hidden />
              {tm('actions.unfollow')}
            </RowMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );

  let body;
  if ((isError && !detail) || (detail && current === null)) {
    // Yahoo knows nothing of the ticker, or gave no price: said so, not a blank page. A refetch
    // that fails behind a page already loaded leaves that page as it is.
    body = <ScaledEmptyState icon={<Eye />} title={tm('detail.noData')} description={ticker} />;
  } else if (isLoading || !detail || current === null) {
    body = <ClassSkeleton />;
  } else {
    const hasTarget = targetValue !== null;
    const targetState = watchTargetState(current, targetValue, detail.targetDirection);
    const price = (value: number) => watchPriceText(fmt, value, currency);

    // The change over the shown line; "vs 3. 10." when it starts later than its period
    const periodChange = points.length > 1 ? changeFromReference(current, points[0].value) : null;
    const changeBase = trendChangeBase(points, chartPeriodStart(period, nowSec));
    const changeLabel =
      changeBase === null
        ? t(`overview.change.${period}`)
        : td('changeSince', { date: fmt.day(changeBase, changeSinceDayOptions(changeBase)) });

    const today = dayChange(detail.currentPrice, detail.previousClose);
    const distance = targetDistance(detail.currentPrice, detail.targetPrice);
    const watchPrice =
      followedAt !== null && sinceSeries ? watchPriceAt(sinceSeries, followedAt) : null;
    const sinceChange = watchPrice !== null ? changeFromReference(current, watchPrice) : null;

    // The tone and the sign follow the percent as it is written, so a change that rounds to zero is neutral
    const todayPct = shownPercent(today?.pct ?? null);
    const sincePct = shownPercent(sinceChange?.pct ?? null);

    const stats: StatItem[] = [
      {
        label: t('wealth.watch.stats.today'),
        value: todayPct === null ? '—' : fmt.percent(todayPct / 100, 1, { signed: true }),
        tone: statTone(todayPct),
        note: today ? watchPriceText(fmt, today.abs, currency, true) : undefined,
      },
      {
        label: tm('table.target'),
        value: targetValue !== null ? price(targetValue) : '—',
        note:
          targetState === 'reached'
            ? t('wealth.stocks.targetReached')
            : targetState === 'pending'
              ? distance !== null
                ? tm('detail.toTarget', { pct: fmt.percent(Math.abs(distance) / 100, 1) })
                : undefined
              : followed
                ? t('wealth.watch.stats.noTarget')
                : undefined,
      },
      {
        label: t('wealth.watch.stats.since'),
        value: sincePct === null ? '—' : fmt.percent(sincePct / 100, 1, { signed: true }),
        tone: statTone(sincePct),
        note:
          followedAt !== null
            ? fmt.day(followedAt, shortDayOptions(followedAt))
            : tm('detail.notFollowedShort'),
      },
    ];

    body = (
      <>
        {/* The nav bar keeps its one title, so the name and the exchange are the first line */}
        <p className="mx-0.5 -mt-0.5 mb-2.5 truncate text-center text-m-caption text-ink-3">
          {[name, detail.exchange ? exchangeName(detail.exchange) : null]
            .filter(Boolean)
            .join(' · ')}
        </p>
        <Hero
          label={tm('detail.price')}
          value={current}
          formatValue={price}
          change={
            periodChange
              ? {
                  // Rounded as the badge writes it: a change of none is neutral, not a gain
                  ratio: (shownPercent(periodChange.pct) ?? 0) / 100,
                  text: `${watchPriceText(fmt, periodChange.abs, currency, true)} ${changeLabel}`,
                }
              : undefined
          }
          note={series === undefined ? undefined : tm('chart.noData')}
          period={period}
          periods={CHART_PERIODS}
          onPeriodChange={setPeriod}
          points={points}
          cost={hasTarget}
          zeroBaseline={period === 'All'}
          chartLabel={`${detail.ticker} · ${tm('detail.price')}`}
        >
          {points.length > 1 && (
            <ScaledChartLegend
              items={[
                { label: tm('detail.legend.price'), swatch: { kind: 'line' } },
                ...(hasTarget
                  ? [{ label: tm('detail.legend.target'), swatch: { kind: 'dash' as const } }]
                  : []),
              ]}
            />
          )}
        </Hero>
        <ButtonPair
          primary={
            followed
              ? {
                  label: hasTarget ? tm('actions.editTarget') : tm('actions.setTarget'),
                  icon: Target,
                  onClick: () => setTargetOpen(true),
                }
              : {
                  label: tm('actions.follow'),
                  icon: Eye,
                  onClick: () => followMutation.mutate(detail.ticker),
                  disabled: followMutation.isPending,
                }
          }
          secondary={{ label: ts('detail.buy'), icon: Plus, onClick: () => setBuyOpen(true) }}
        />
        <StatRow stats={stats} />
        {followed && (
          <WatchNotes
            notes={detail.notes}
            saving={notesMutation.isPending}
            onSave={async (notes) => {
              await notesMutation.mutateAsync({ ticker: detail.ticker, notes });
              // The refetch the save triggers lands after this resolves: the card leaves the editor
              // on the saved text, not on the old one for the time it takes
              queryClient.setQueryData<StockMonitorDetail>(
                ['stock-monitor', 'detail', ticker],
                (old) => (old ? { ...old, notes } : old)
              );
            }}
          />
        )}
        <WatchKeyData detail={detail} />
      </>
    );
  }

  return (
    <Screen
      title={ticker}
      leading={<NavTextButton label={t('wealth.stocks.back')} onClick={back} chevron />}
      trailing={menu}
    >
      {body}
      {detail && (
        <>
          <WatchTargetSheet
            open={targetOpen}
            onOpenChange={setTargetOpen}
            ticker={detail.ticker}
            currency={currency}
            currentPrice={current}
            high52={priceOf(detail.fiftyTwoWeekHigh)}
            targetPrice={detail.targetPrice}
            targetDirection={detail.targetDirection}
          />
          <AddInvestmentModal open={buyOpen} onOpenChange={setBuyOpen} initial={buyInitial} />
        </>
      )}
      <ConfirmSheet
        open={confirmUnfollow}
        onOpenChange={setConfirmUnfollow}
        title={tm('unfollowConfirm.title', { ticker })}
        description={tm('unfollowConfirm.description')}
        confirmLabel={tm('actions.unfollow')}
        isPending={unfollowMutation.isPending}
        onConfirm={() =>
          unfollowMutation.mutate(ticker, {
            // The sheet takes its own history entry off first, then the page goes back to Akcie
            onSuccess: () => leaveAfterSheet(() => setConfirmUnfollow(false), back),
          })
        }
      />
    </Screen>
  );
}
