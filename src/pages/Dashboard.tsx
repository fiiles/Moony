import { useMemo, useState } from 'react';
import { useLocation } from 'wouter';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { subDays, startOfYear } from 'date-fns';
import { Landmark, Plus, Target, Wallet } from 'lucide-react';
import type { PortfolioMetricsHistory } from '@shared/schema';
import { portfolioApi } from '@/lib/tauri-api';
import { useAuth } from '@/hooks/use-auth';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useLanguage } from '@/i18n/I18nProvider';
import { useHistoricalDisplayValues } from '@/hooks/use-historical-display-values';
import { useRecentMoves } from '@/hooks/use-recent-moves';
import { utcDayFloor } from '@/utils/chart-axis';
import { vocative } from '@/utils/vocative';
import TimePeriodSelector, { type Period } from '@/components/cashflow/TimePeriodSelector';
import { PageHead, SectionHead } from '@/components/shell/PageHead';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { SeriesDot, Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { EmptyState } from '@/components/common/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { GettingStartedCard } from '@/components/dashboard/GettingStartedCard';
import { AllocationRing } from '@/components/dashboard/AllocationRing';
import { shortHistoryBaseDay } from '@/utils/change-base';
import { RecentMoves } from '@/components/dashboard/RecentMoves';

/** The overview offers the four periods of the prototype (design system §7). */
const PERIODS: readonly Period[] = ['30D', '90D', '1Y', 'All'];

type AssetClass = 'investments' | 'savings' | 'bonds' | 'crypto' | 'otherAssets' | 'realEstate';
const ASSET_CLASSES: AssetClass[] = [
  'investments',
  'savings',
  'bonds',
  'crypto',
  'otherAssets',
  'realEstate',
];

/** Greeting by the local hour: morning until 11, day until 18, evening after. */
function greetingKey(hour: number): 'morning' | 'day' | 'evening' {
  if (hour >= 5 && hour < 11) return 'morning';
  if (hour >= 11 && hour < 18) return 'day';
  return 'evening';
}

export default function Dashboard() {
  const { t } = useTranslation('dashboard');
  const [, setLocation] = useLocation();
  const { user } = useAuth();
  const fmt = useFormat();
  const { getLocale } = useLanguage();
  const { ratesTimestamp, formatCurrencyRaw, formatCurrency, currencyCode } = useCurrency();
  const [selectedPeriod, setSelectedPeriod] = useState<Period>('30D');

  // First visit right after onboarding greets with "Welcome", later visits by
  // the time of day. The flag is consumed on first read.
  const [isFirstVisit] = useState(() => {
    try {
      const first = localStorage.getItem('moony-first-run') === '1';
      if (first) localStorage.removeItem('moony-first-run');
      return first;
    } catch {
      return false;
    }
  });
  const name = vocative(user?.name ?? '', getLocale());
  const greeting = isFirstVisit
    ? t('welcomeFirst', { name })
    : t(`greeting.${greetingKey(new Date().getHours())}`, { name });
  const asOf = t('asOf', {
    date: fmt.date(new Date(), { day: 'numeric', month: 'long', year: 'numeric' }),
  });

  // Date range of the selected period
  const dateRange = useMemo(() => {
    const now = new Date();
    switch (selectedPeriod) {
      case '30D':
        return { start: subDays(now, 30), end: now };
      case '90D':
        return { start: subDays(now, 90), end: now };
      case 'YTD':
        return { start: startOfYear(now), end: now };
      case '1Y':
        return { start: subDays(now, 365), end: now };
      case '5Y':
        return { start: subDays(now, 365 * 5), end: now };
      case 'All':
        return { start: undefined, end: now };
      default:
        return { start: subDays(now, 30), end: now };
    }
  }, [selectedPeriod]);

  const { data: portfolioHistory } = useQuery<PortfolioMetricsHistory[]>({
    queryKey: ['portfolio-history', dateRange.start?.toISOString(), dateRange.end.toISOString()],
    queryFn: async () => {
      const startDate = dateRange.start ? Math.floor(dateRange.start.getTime() / 1000) : undefined;
      const endDate = Math.floor(dateRange.end.getTime() / 1000);
      return portfolioApi.getHistory(startDate, endDate);
    },
    staleTime: 0,
    refetchOnMount: 'always',
  });

  // ratesTimestamp in the key: re-fetch whenever ECB rates refresh, so the
  // overview uses the same rates as the stocks/crypto pages.
  const {
    data: portfolioMetrics,
    isLoading: metricsLoading,
    isFetching: metricsFetching,
  } = useQuery({
    queryKey: ['portfolio-metrics', ratesTimestamp],
    queryFn: async () => portfolioApi.getMetrics(user?.excludePersonalRealEstate || false),
    staleTime: 0,
    refetchOnMount: 'always',
  });

  const totals = {
    savings: portfolioMetrics?.totalSavings || 0,
    investments: portfolioMetrics?.totalInvestments || 0,
    bonds: portfolioMetrics?.totalBonds || 0,
    realEstate: portfolioMetrics?.totalRealEstate || 0,
    crypto: portfolioMetrics?.totalCrypto || 0,
    otherAssets: portfolioMetrics?.totalOtherAssets || 0,
  };
  const totalAssets = portfolioMetrics?.totalAssets || 0;
  const totalLiabilities = portfolioMetrics?.totalLiabilities || 0;
  const netWorth = portfolioMetrics?.netWorth || 0;

  // Brand-new profile: every total is zero. The empty state is only shown once
  // the metrics have settled, so it never flashes during a refetch.
  const allTotalsZero =
    portfolioMetrics !== undefined &&
    [...Object.values(totals), totalAssets, totalLiabilities].every((value) => value === 0);
  const isCheckingPortfolio = metricsLoading || (allTotalsZero && metricsFetching);
  const isEmptyPortfolio = allTotalsZero && !metricsFetching;

  // Per-day display-currency conversion shared by the hero chart and the stats
  const { convertPoint, convertCzkPoint, convertCzkToday } =
    useHistoricalDisplayValues(portfolioHistory);

  // History in chronological order, converted with each day's rates. When
  // personal real estate is excluded, that class converts via its CZK total
  // because the stored breakdown still includes the personal part.
  const convertedHistory = useMemo(() => {
    const excludePersonal = user?.excludePersonalRealEstate || false;
    return [...(portfolioHistory || [])].reverse().map((h) => {
      const savings = convertPoint(h.recordedAt, h.savingsByCurrency, Number(h.totalSavings));
      const investments = convertPoint(
        h.recordedAt,
        h.investmentsByCurrency,
        Number(h.totalInvestments)
      );
      const bonds = convertPoint(h.recordedAt, h.bondsByCurrency, Number(h.totalBonds));
      const crypto = convertPoint(h.recordedAt, h.cryptoByCurrency, Number(h.totalCrypto || 0));
      const otherAssets = convertPoint(
        h.recordedAt,
        h.otherAssetsByCurrency,
        Number(h.totalOtherAssets || 0)
      );
      const realEstate = excludePersonal
        ? convertCzkPoint(h.recordedAt, Number(h.totalRealEstateInvestment))
        : convertPoint(
            h.recordedAt,
            h.realEstateByCurrency,
            Number(h.totalRealEstatePersonal) + Number(h.totalRealEstateInvestment)
          );
      const liabilities = convertPoint(
        h.recordedAt,
        h.loansByCurrency,
        Number(h.totalLoansPrincipal)
      );
      const assets = savings + investments + bonds + realEstate + crypto + otherAssets;
      return {
        recordedAt: h.recordedAt,
        savings,
        investments,
        bonds,
        crypto,
        otherAssets,
        realEstate,
        assets,
        liabilities,
        netWorth: assets - liabilities,
      };
    });
  }, [portfolioHistory, convertPoint, convertCzkPoint, user?.excludePersonalRealEstate]);

  // Live "today" values at today's rates: the last chart point equals the hero number
  const display = {
    netWorth: convertCzkToday(netWorth),
    assets: convertCzkToday(totalAssets),
    liabilities: convertCzkToday(totalLiabilities),
    savings: convertCzkToday(totals.savings),
    investments: convertCzkToday(totals.investments),
    bonds: convertCzkToday(totals.bonds),
    realEstate: convertCzkToday(totals.realEstate),
    crypto: convertCzkToday(totals.crypto),
    otherAssets: convertCzkToday(totals.otherAssets),
  };

  const oldest = convertedHistory[0];
  // What the change is measured against: the period, or — when the
  // history is shorter than the range — the oldest recorded day.
  const periodStartSeconds = dateRange.start
    ? Math.floor(dateRange.start.getTime() / 1000)
    : undefined;
  const shortHistoryBase = shortHistoryBaseDay(periodStartSeconds, oldest?.recordedAt);
  const changeLabel =
    shortHistoryBase === null
      ? t(`changePeriod.${selectedPeriod}`)
      : t('changeSince', {
          date: fmt.day(shortHistoryBase, {
            month: 'short',
            day: 'numeric',
            year:
              new Date(shortHistoryBase * 1000).getUTCFullYear() !== new Date().getUTCFullYear()
                ? 'numeric'
                : undefined,
          }),
        });
  const hasHistory = convertedHistory.length > 1;
  const netWorthDelta = oldest ? display.netWorth - oldest.netWorth : 0;
  const netWorthPct =
    oldest && oldest.netWorth !== 0 ? netWorthDelta / Math.abs(oldest.netWorth) : 0;
  const assetsDelta = oldest ? display.assets - oldest.assets : 0;
  const liabilitiesDelta = oldest ? display.liabilities - oldest.liabilities : 0;

  // Insight: the asset class with the largest absolute contribution to the change
  const insight = useMemo(() => {
    if (!oldest || !hasHistory) return t('insight.noHistory');
    const deltas: { cls: AssetClass | 'liabilities'; delta: number }[] = ASSET_CLASSES.map(
      (cls) => ({ cls, delta: display[cls] - oldest[cls] })
    );
    deltas.push({ cls: 'liabilities', delta: -(display.liabilities - oldest.liabilities) });
    const top = deltas.reduce((a, b) => (Math.abs(b.delta) > Math.abs(a.delta) ? b : a));
    const cls = t(`classes.${top.cls}`);
    if (netWorthPct > 0.005) return t('insight.up', { class: cls });
    if (netWorthPct < -0.005) return t('insight.down', { class: cls });
    return t('insight.flat', { class: cls });
    // display is rebuilt every render; its inputs are the metric totals below
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [oldest, hasHistory, netWorthPct, t, portfolioMetrics, ratesTimestamp]);

  // Hero chart: history plus the live value for today (UTC days, ADR 0008)
  const points = useMemo(() => {
    const todayStart = utcDayFloor(Date.now() / 1000);
    const pts = convertedHistory.map((h) => ({ t: h.recordedAt, value: h.netWorth }));
    const last = pts[pts.length - 1];
    if (last && utcDayFloor(last.t) === todayStart) last.value = display.netWorth;
    else pts.push({ t: Math.floor(Date.now() / 1000), value: display.netWorth });
    return pts;
  }, [convertedHistory, display.netWorth]);

  const allocation = ASSET_CLASSES.map((cls) => ({
    key: cls,
    label: t(`cards.${cls}`),
    value: totals[cls],
  }));

  const { moves, isLoading: movesLoading } = useRecentMoves(5);

  const tone = (delta: number) => (delta > 0 ? 'gain' : delta < 0 ? 'loss' : 'neutral');
  const signedMoney = (value: number) =>
    fmt.money(value, currencyCode, { signed: true, decimals: 0 });

  const head = (
    <PageHead
      eyebrow={t('eyebrow')}
      title={greeting}
      description={asOf}
      actions={
        !isEmptyPortfolio && (
          <TimePeriodSelector
            value={selectedPeriod}
            onChange={setSelectedPeriod}
            options={PERIODS}
            size="lg"
            labels="long"
          />
        )
      }
    />
  );

  if (isCheckingPortfolio) {
    return (
      <>
        {head}
        <Card className="px-6 pb-4 pt-6">
          <div className="skeleton h-3 w-24" />
          <div className="skeleton mt-3 h-10 w-64" />
          <div className="skeleton mt-3 h-3 w-48" />
          <div className="skeleton mt-6 h-[170px] w-full" />
        </Card>
        <Stats columns={3} className="mt-[15px]">
          <StatSkeleton />
          <StatSkeleton />
          <StatSkeleton />
        </Stats>
      </>
    );
  }

  if (isEmptyPortfolio) {
    return (
      <>
        {head}
        <EmptyState
          icon={<Wallet />}
          title={t('empty.title')}
          description={t('empty.description')}
          action={
            <div className="flex flex-wrap items-center justify-center gap-2">
              <Button onClick={() => setLocation('/bank-accounts')}>
                <Landmark />
                {t('empty.addAccount')}
              </Button>
              <Button variant="outline" onClick={() => setLocation('/stocks')}>
                <Plus />
                {t('empty.addInvestment')}
              </Button>
              <Button variant="outline" onClick={() => setLocation('/reports/budgeting')}>
                <Target />
                {t('empty.setBudget')}
              </Button>
            </div>
          }
        />
      </>
    );
  }

  return (
    <>
      {head}

      {/* Getting started checklist (hides itself when done or dismissed) */}
      <GettingStartedCard />

      <HeroCard
        aside={
          <p className="m-0 mt-0.5 max-w-[220px] text-right text-caption leading-[1.5] text-ink-4">
            {insight}
          </p>
        }
        chart={
          <MoonyLineChart
            points={points}
            zeroBaseline={selectedPeriod === 'All'}
            height={170}
            className="mt-[18px] -mx-1"
            formatValue={(v) => formatCurrencyRaw(v)}
            renderTip={(p) => ({ title: formatCurrencyRaw(p.value), lines: [fmt.day(p.t)] })}
          />
        }
      >
        <HeroValue
          label={t('hero.label')}
          value={formatCurrencyRaw(display.netWorth)}
          tone={tone(netWorthDelta)}
          delta={
            hasHistory
              ? `${netWorthDelta >= 0 ? '↗' : '↘'} ${fmt.percent(netWorthPct, 1, { signed: true })}`
              : undefined
          }
          deltaNote={
            hasHistory ? `${signedMoney(netWorthDelta)} ${changeLabel}` : t('stats.noHistory')
          }
        />
      </HeroCard>

      <Stats columns={3} className="mt-[15px]">
        <Stat
          label={t('stats.totalAssets')}
          aside={<SeriesDot className="bg-s1" />}
          value={formatCurrency(totalAssets)}
          note={hasHistory ? `${signedMoney(assetsDelta)} ${changeLabel}` : t('stats.noHistory')}
        />
        <Stat
          label={t('stats.totalLiabilities')}
          aside={<SeriesDot className="bg-s3" />}
          value={formatCurrency(totalLiabilities)}
          note={
            !hasHistory
              ? t('stats.noHistory')
              : Math.abs(liabilitiesDelta) < 1
                ? t('stats.unchanged')
                : `${signedMoney(liabilitiesDelta)} ${changeLabel}`
          }
        />
        <Stat
          label={t('stats.freeCash')}
          aside={<SeriesDot className="bg-s4" />}
          value={formatCurrency(totals.savings)}
          note={t('stats.freeCashNote')}
        />
      </Stats>

      <div className="mt-9 grid grid-cols-[1.15fr_0.85fr] gap-[18px]">
        <section className="flex flex-col">
          <SectionHead
            title={t('allocation.title')}
            link={{ href: '/stocks', label: t('allocation.open') }}
          />
          {/* Stretches to the height of "Poslední pohyby"; the ring centers vertically */}
          <Card variant="flat" className="flex flex-1 items-center">
            <CardContent className="w-full pt-5">
              <AllocationRing
                segments={allocation}
                centerLabel={t('allocation.center')}
                centerValue={fmt.number(display.assets, {
                  notation: 'compact',
                  maximumFractionDigits: 2,
                })}
                otherLabel={t('cards.other')}
                formatValue={(v) => formatCurrency(v)}
                formatPercent={(r) => fmt.percent(r, 0)}
              />
            </CardContent>
          </Card>
        </section>
        <section className="flex flex-col">
          <SectionHead
            title={t('moves.title')}
            link={{ href: '/bank-accounts', label: t('moves.all') }}
          />
          <Card variant="flat" className="flex-1">
            <CardContent className="pb-2 pt-2">
              <RecentMoves moves={moves} isLoading={movesLoading} />
            </CardContent>
          </Card>
        </section>
      </div>
    </>
  );
}
