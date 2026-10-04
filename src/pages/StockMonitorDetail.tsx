import { useMemo, useState } from 'react';
import { Link, useRoute } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Eye, Plus, RefreshCw } from 'lucide-react';
import { currencyDecimals } from '@shared/currencies';
import { exchangeName } from '@/utils/exchange-names';
import { useFormat } from '@/lib/use-format';
import { useShellPage } from '@/components/shell/shell-context';
import { useStockMonitorDetail, useStockPriceRange } from '@/hooks/use-stock-monitor';
import { useStockMonitorMutations } from '@/hooks/use-stock-monitor-mutations';
import {
  CHART_PERIODS,
  changeFromReference,
  dayChange,
  formatMarketCap,
  formatNativePrice,
  percentFromFraction,
  targetDistance,
  type ChartPeriod,
} from '@/utils/stock-monitor';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import { BackLink, PageHead } from '@/components/shell/PageHead';
import { Button } from '@/components/ui/button';
import { Segmented } from '@/components/ui/segmented';
import { Card, CardHeader, CardTitle } from '@/components/ui/card';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { EmptyState } from '@/components/common/EmptyState';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { AXIS_WIDTH } from '@/components/charts/ChartAxis';
import { StockNotesCard } from '@/components/stock-monitor/StockNotesCard';
import { RangeWithLabels } from '@/components/stock-monitor/RangeBar';
import {
  TargetPriceDialog,
  type TargetPriceTarget,
} from '@/components/stock-monitor/TargetPriceDialog';
import { AddInvestmentModal } from '@/components/stocks/AddInvestmentModal';
import { cn } from '@/lib/utils';

const DAY = 86_400;

/** Chart range that surely contains the day the stock was followed. */
function periodCovering(followedAt: number): ChartPeriod {
  const age = (Date.now() / 1000 - followedAt) / DAY;
  if (age <= 28) return '1M';
  if (age <= 180) return '6M';
  if (age <= 365) return '1Y';
  if (age <= 5 * 365) return '5Y';
  return 'MAX';
}

/**
 * Watched stock detail (design system §7 Watchlist, prototype
 * `stock-monitor-detail.html`): native price hero with the target line and
 * the watch-start marker, four stats with the 52-week range bar, markdown
 * notes, key data and the change since the stock was followed.
 */
export default function StockMonitorDetail() {
  const { t } = useTranslation('stockMonitor');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const locale = fmt.locale;
  const [, params] = useRoute('/stock-monitor/:ticker');
  const ticker = params?.ticker ? decodeURIComponent(params.ticker) : undefined;

  const { data: detail, isLoading, isError } = useStockMonitorDetail(ticker);
  const {
    followMutation,
    unfollowMutation,
    targetPriceMutation,
    notesMutation,
    refreshDetailMutation,
  } = useStockMonitorMutations();

  const [period, setPeriod] = useState<ChartPeriod>('6M');
  const [target, setTarget] = useState<TargetPriceTarget | null>(null);
  const [confirmUnfollow, setConfirmUnfollow] = useState(false);
  const [buyOpen, setBuyOpen] = useState(false);

  const { data: range = [], isLoading: rangeLoading } = useStockPriceRange(ticker, period);
  const followedAt = detail?.followedAt ?? null;
  const sincePeriod = followedAt ? periodCovering(followedAt) : period;
  const { data: sinceRange = [] } = useStockPriceRange(
    followedAt ? ticker : undefined,
    sincePeriod
  );

  const name = detail?.longName ?? detail?.shortName ?? detail?.ticker ?? ticker ?? '';
  const currency = detail?.currency ?? null;
  const current = parseFloat(detail?.currentPrice ?? '');
  const prevClose = parseFloat(detail?.previousClose ?? '');
  const targetValue = parseFloat(detail?.targetPrice ?? '');
  const hasTarget = isFinite(targetValue);
  const low = parseFloat(detail?.fiftyTwoWeekLow ?? '');
  const high = parseFloat(detail?.fiftyTwoWeekHigh ?? '');
  const fetchedAt = detail?.priceFetchedAt ?? null;

  useShellPage({
    crumb: detail ? name : undefined,
    status:
      fetchedAt != null
        ? { text: t('status.updatedShort', { time: fmt.time(fetchedAt) }), tone: 'fresh' }
        : undefined,
  });

  const price = (v: number) => formatNativePrice(v, currency, locale);
  const plain = (n: number) => {
    const d = currencyDecimals(currency);
    return fmt.number(n, { minimumFractionDigits: d, maximumFractionDigits: d });
  };
  const intraday = period === '1D' || period === '5D';

  const points = useMemo(() => {
    const pts = range.map((p) => ({ t: p.timestamp, value: p.price }));
    if (isFinite(current) && pts.length > 0) {
      const last = pts[pts.length - 1];
      const at = fetchedAt ?? Math.floor(Date.now() / 1000);
      if (at > last.t) pts.push({ t: at, value: current });
      else last.value = current;
    }
    return pts;
  }, [range, current, fetchedAt]);

  const watchEvents = useMemo<ChartEvent[]>(
    () =>
      followedAt && points.length > 0 && points[0].t <= followedAt
        ? [{ id: 'watch-start', t: followedAt, type: 'expense' }]
        : [],
    [followedAt, points]
  );

  const priceAt = (series: { timestamp: number; price: number }[], at: number) => {
    if (series.length === 0) return null;
    let best = series[0];
    for (const p of series)
      if (Math.abs(p.timestamp - at) < Math.abs(best.timestamp - at)) best = p;
    return best.price;
  };
  const watchPrice = followedAt ? priceAt(sinceRange, followedAt) : null;

  const periodStart = points[0]?.value;
  const periodChange =
    periodStart !== undefined && isFinite(current)
      ? changeFromReference(current, periodStart)
      : null;
  const today = dayChange(detail?.currentPrice ?? null, detail?.previousClose ?? null);
  const distance = targetDistance(detail?.currentPrice ?? null, detail?.targetPrice ?? null);

  const periodLabel = (p: ChartPeriod) =>
    p === '1Y' ? tc('periods.1Y') : p === '5Y' ? tc('periods.5Y') : p === 'MAX' ? 'Max' : p;

  // Intraday axis: times instead of days
  const intradayLabels = useMemo(() => {
    if (!intraday || points.length < 2) return [];
    const t0 = points[0].t;
    const t1 = points[points.length - 1].t;
    return Array.from({ length: 5 }, (_, i) => {
      const ts = t0 + ((t1 - t0) * i) / 4;
      return period === '1D'
        ? fmt.time(ts)
        : fmt.date(new Date(ts * 1000), { day: 'numeric', month: 'numeric' });
    });
  }, [intraday, points, period, fmt]);

  const openTarget = () =>
    detail &&
    setTarget({
      ticker: detail.ticker,
      currency: detail.currency,
      currentPrice: detail.currentPrice,
      fiftyTwoWeekHigh: detail.fiftyTwoWeekHigh,
      targetPrice: detail.targetPrice,
    });

  if (isError) {
    return (
      <>
        <BackLink href="/stock-monitor">{t('detail.back')}</BackLink>
        <EmptyState icon={<Eye />} title={t('detail.noData')} description={ticker ?? ''} />
      </>
    );
  }

  if (isLoading || !detail) {
    return (
      <>
        <BackLink href="/stock-monitor">{t('detail.back')}</BackLink>
        <PageHead title={ticker ?? ''} />
        <Stats>
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      </>
    );
  }

  const dividendPct = percentFromFraction(detail.trailingDividendYield);
  const dividendPerShare =
    dividendPct !== null && isFinite(current) ? (current * dividendPct) / 100 : null;
  const marketCap = formatMarketCap(detail.marketCap, currency, locale);
  const peRatio = parseFloat(detail.peRatio ?? '');
  const sinceDays = followedAt
    ? Math.max(0, Math.floor((Date.now() / 1000 - followedAt) / DAY))
    : 0;
  const sinceChange =
    watchPrice !== null && isFinite(current) ? changeFromReference(current, watchPrice) : null;

  const kv: { label: string; value: string | null }[] = [
    { label: t('detail.kv.previousClose'), value: isFinite(prevClose) ? price(prevClose) : null },
    {
      label: t('detail.kv.changeToday'),
      value: today ? formatNativePrice(today.abs, currency, locale, undefined, true) : null,
    },
    { label: t('detail.kv.low'), value: isFinite(low) ? price(low) : null },
    { label: t('detail.kv.high'), value: isFinite(high) ? price(high) : null },
    { label: t('detail.kv.marketCap'), value: marketCap },
    {
      label: t('detail.kv.pe'),
      value: isFinite(peRatio) ? fmt.number(peRatio, { maximumFractionDigits: 1 }) : null,
    },
    {
      label: t('detail.kv.dividend'),
      value: dividendPct !== null ? fmt.percent(dividendPct / 100, 2) : null,
    },
    {
      label: t('detail.kv.exchange'),
      value: detail.exchange ? exchangeName(detail.exchange) : null,
    },
    { label: t('detail.kv.updated'), value: fetchedAt != null ? fmt.dateTime(fetchedAt) : null },
  ];

  return (
    <>
      <BackLink href="/stock-monitor">{t('detail.back')}</BackLink>
      <PageHead
        eyebrow={
          [detail.ticker, detail.exchange ? exchangeName(detail.exchange) : null, currency].filter(
            Boolean
          ) as string[]
        }
        title={name}
        description={
          detail.followed
            ? `${followedAt ? t('detail.followedSince', { date: fmt.day(followedAt) }) : ''}${followedAt ? ' · ' : ''}${
                detail.isHeld ? t('detail.held') : t('detail.notHeld')
              }`
            : t('detail.notFollowedShort')
        }
        actions={
          <>
            <Button
              variant="outline"
              onClick={() => refreshDetailMutation.mutate(detail.ticker)}
              loading={refreshDetailMutation.isPending}
              data-testid="refresh-detail"
            >
              <RefreshCw />
              {t('actions.refreshShort')}
            </Button>
            {detail.followed && (
              <Button variant="outline" onClick={openTarget}>
                {hasTarget ? t('actions.editTarget') : t('actions.setTarget')}
              </Button>
            )}
            {detail.followed ? (
              <Button variant="danger" onClick={() => setConfirmUnfollow(true)}>
                {t('actions.unfollow')}
              </Button>
            ) : (
              <Button
                onClick={() => followMutation.mutate(detail.ticker)}
                loading={followMutation.isPending}
                data-testid="follow-button"
              >
                <Eye />
                {t('actions.follow')}
              </Button>
            )}
            {detail.isHeld && detail.heldInvestmentId ? (
              <Button variant="outline" asChild>
                <Link href={`/stocks/${detail.heldInvestmentId}`}>
                  {t('actions.openInPortfolio')}
                </Link>
              </Button>
            ) : (
              <Button
                variant={detail.followed ? 'default' : 'outline'}
                onClick={() => setBuyOpen(true)}
              >
                <Plus />
                {t('actions.buy')}
              </Button>
            )}
          </>
        }
      />

      <HeroCard
        aside={
          <Segmented
            value={period}
            onValueChange={setPeriod}
            options={CHART_PERIODS.map((p) => ({ value: p, label: periodLabel(p) }))}
            aria-label={tc('periods.label')}
          />
        }
        chart={
          <>
            {rangeLoading ? (
              <div className="skeleton mt-[18px] h-[230px] rounded-r2" />
            ) : points.length === 0 ? (
              <p className="grid h-[230px] place-items-center text-table text-ink-3">
                {t('chart.noData')}
              </p>
            ) : (
              <MoonyLineChart
                className="-mx-1 mt-[18px]"
                points={points}
                height={230}
                formatValue={price}
                formatTick={(v) => plain(v)}
                hideAxis={intraday}
                reference={
                  hasTarget
                    ? {
                        value: targetValue,
                        label: t('chart.targetLabel', { price: plain(targetValue) }),
                        color: 'var(--gain)',
                      }
                    : undefined
                }
                events={watchEvents}
                renderTip={(p) => ({
                  title: price(p.value),
                  lines: [intraday ? fmt.dateTime(p.t) : fmt.day(p.t)],
                })}
                renderEventTip={(cluster: EventCluster) => ({
                  title: t('detail.legend.watchStart'),
                  lines: [
                    [
                      fmt.day(cluster.t),
                      watchPrice !== null ? price(watchPrice) : null,
                      hasTarget
                        ? t('detail.watchStartTarget', { price: price(targetValue) })
                        : null,
                    ]
                      .filter(Boolean)
                      .join(' · '),
                  ],
                })}
              />
            )}
            {intraday && intradayLabels.length > 0 && (
              <div
                aria-hidden
                className="mt-1.5 flex justify-between text-micro font-500 text-chart-axis"
                style={{ paddingRight: AXIS_WIDTH }}
              >
                {intradayLabels.map((l, i) => (
                  <span key={i}>{l}</span>
                ))}
              </div>
            )}
            <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
              <ChartLegend
                items={[
                  { label: t('detail.legend.price'), swatch: { kind: 'line' } },
                  ...(hasTarget
                    ? [
                        {
                          label: t('detail.legend.target'),
                          swatch: { kind: 'dash' as const, color: 'var(--gain)' },
                        },
                      ]
                    : []),
                  ...(watchEvents.length > 0
                    ? [{ label: t('detail.legend.watchStart'), swatch: { kind: 'ring' as const } }]
                    : []),
                ]}
              />
              {periodStart !== undefined && periodChange && (
                <span className="mt-[14px] text-micro font-500 text-ink-4 num">
                  {t('detail.periodStart', {
                    period: periodLabel(period),
                    price: price(periodStart),
                  })}{' '}
                  ·{' '}
                  {t('detail.periodChange', {
                    change: fmt.percent(periodChange.pct / 100, 1, { signed: true }),
                  })}
                </span>
              )}
            </div>
          </>
        }
      >
        <HeroValue
          label={t('detail.price')}
          value={isFinite(current) ? price(current) : '—'}
          tone={today ? (today.abs >= 0 ? 'gain' : 'loss') : 'neutral'}
          delta={
            today
              ? `${today.abs >= 0 ? '↗' : '↘'} ${formatNativePrice(today.abs, currency, locale, undefined, true)}`
              : undefined
          }
          deltaNote={
            today && isFinite(prevClose)
              ? t('detail.today', {
                  change: fmt.percent(today.pct / 100, 1, { signed: true }),
                  close: plain(prevClose),
                })
              : undefined
          }
        />
        <HeroValue
          compact
          label={t('detail.target')}
          value={hasTarget ? price(targetValue) : '—'}
          deltaNote={
            hasTarget ? (
              distance !== null && distance > 0 ? (
                t('detail.toTarget', { pct: fmt.percent(distance / 100, 1) })
              ) : (
                <span className="text-gain">{t('detail.targetReached')}</span>
              )
            ) : detail.followed ? (
              <button
                type="button"
                className="font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
                onClick={openTarget}
              >
                {t('actions.setTarget')} →
              </button>
            ) : undefined
          }
        />
      </HeroCard>

      <Stats className="mt-[17px]">
        <Stat
          label={t('detail.marketCap')}
          value={marketCap ?? '—'}
          note={
            detail.exchange
              ? t('detail.stats.exchangeNote', { exchange: exchangeName(detail.exchange) })
              : undefined
          }
        />
        <Stat
          label={t('detail.peRatio')}
          value={isFinite(peRatio) ? fmt.number(peRatio, { maximumFractionDigits: 1 }) : '—'}
          note={t('detail.stats.peNote')}
        />
        <Stat
          label={t('detail.dividendYield')}
          value={dividendPct !== null ? fmt.percent(dividendPct / 100, 2) : '—'}
          note={
            dividendPerShare !== null && dividendPerShare > 0
              ? t('detail.stats.dividendNote', { amount: price(dividendPerShare) })
              : undefined
          }
        />
        <Stat
          label={t('detail.stats.range')}
          value={
            isFinite(low) && isFinite(high) ? (
              <span className="text-[16px]">
                {plain(low)} – {plain(high)} {currency ?? ''}
              </span>
            ) : (
              '—'
            )
          }
          note={
            isFinite(low) && isFinite(high) && isFinite(current) ? (
              <RangeWithLabels
                fluid
                className="mt-1"
                low={low}
                high={high}
                current={current}
                target={hasTarget ? targetValue : null}
                lowLabel={t('detail.stats.min')}
                highLabel={t('detail.stats.max')}
                label={
                  hasTarget
                    ? t('detail.stats.rangeLabelTarget', {
                        low: plain(low),
                        high: plain(high),
                        current: plain(current),
                        target: plain(targetValue),
                      })
                    : t('detail.stats.rangeLabel', {
                        low: plain(low),
                        high: plain(high),
                        current: plain(current),
                      })
                }
              />
            ) : undefined
          }
        />
      </Stats>

      <div className="grid grid-cols-[1.5fr_1fr] items-start gap-[18px]">
        {detail.followed ? (
          <StockNotesCard
            notes={detail.notes}
            saving={notesMutation.isPending}
            onSave={(notes) => notesMutation.mutateAsync({ ticker: detail.ticker, notes })}
          />
        ) : (
          <Card variant="flat">
            <EmptyState
              bare
              icon={<Eye />}
              title={t('detail.notFollowedShort')}
              description={t('detail.followPrompt')}
              action={
                <Button
                  onClick={() => followMutation.mutate(detail.ticker)}
                  loading={followMutation.isPending}
                >
                  <Eye />
                  {t('actions.follow')}
                </Button>
              }
            />
          </Card>
        )}
        <div className="grid gap-[14px]">
          <Card variant="flat">
            <CardHeader className="pb-1">
              <CardTitle className="text-[16px]">{t('detail.keyData')}</CardTitle>
            </CardHeader>
            <div className="px-[21px] pb-4 pt-1">
              {kv.map((row) => (
                <div
                  key={row.label}
                  className="grid grid-cols-[1fr_auto] gap-x-4 border-b border-line-soft py-[9px] text-table text-ink-3 last:border-b-0"
                >
                  <span>{row.label}</span>
                  <b className={cn('font-600 num', row.value ? 'text-ink' : 'text-ink-5')}>
                    {row.value ?? '—'}
                  </b>
                </div>
              ))}
            </div>
          </Card>
          {detail.followed && followedAt && (
            <Card variant="flat" className="px-[21px] py-[18px]">
              <h3 className="m-0 text-h3 text-ink">{t('detail.since.title')}</h3>
              <p className="mt-1.5 text-caption text-ink-3">
                {sinceChange && watchPrice !== null ? (
                  <>
                    {t('detail.since.text', {
                      days: t('detail.since.days', { count: sinceDays }),
                      price: price(watchPrice),
                    })}{' '}
                    <b className={cn('font-650', sinceChange.abs >= 0 ? 'text-gain' : 'text-loss')}>
                      {fmt.percent(sinceChange.pct / 100, 1, { signed: true })}
                    </b>
                    .
                    {hasTarget && isFinite(current) && targetValue > current && (
                      <> {t('detail.since.toTarget', { amount: price(targetValue - current) })}</>
                    )}
                  </>
                ) : (
                  t('detail.since.noData', { days: t('detail.since.days', { count: sinceDays }) })
                )}
              </p>
            </Card>
          )}
        </div>
      </div>

      <TargetPriceDialog
        target={target}
        onClose={() => setTarget(null)}
        onSave={(tk, targetPrice) => targetPriceMutation.mutateAsync({ ticker: tk, targetPrice })}
        saving={targetPriceMutation.isPending}
      />
      <ConfirmDeleteDialog
        open={confirmUnfollow}
        onOpenChange={setConfirmUnfollow}
        title={t('unfollowConfirm.title', { ticker: detail.ticker })}
        description={t('unfollowConfirm.description')}
        confirmLabel={t('actions.unfollow')}
        isPending={unfollowMutation.isPending}
        onConfirm={() =>
          unfollowMutation.mutate(detail.ticker, { onSuccess: () => setConfirmUnfollow(false) })
        }
      />
      <AddInvestmentModal
        open={buyOpen}
        onOpenChange={setBuyOpen}
        initial={{ ticker: detail.ticker, companyName: name, currency }}
      />
    </>
  );
}
