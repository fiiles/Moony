import { useMemo, useState } from 'react';
import { useLocation } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Landmark, Plus, Target, Wallet } from 'lucide-react';
import { useAuth } from '@/hooks/use-auth';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useLanguage } from '@/i18n/I18nProvider';
import { usePortfolioHistory, usePortfolioMetrics } from '@/hooks/use-portfolio';
import { useRecentMoves } from '@/hooks/use-recent-moves';
import { amountOf } from '@/utils/positions';
import { mergeConversionQuality } from '@/utils/conversion-quality';
import { utcDayFloor } from '@/utils/chart-axis';
import { CHART_PERIODS, chartPeriodStart } from '@/utils/period';
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
import { UpcomingCard } from '@/components/dashboard/UpcomingCard';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';

type AssetClass = 'investments' | 'savings' | 'bonds' | 'crypto' | 'otherAssets' | 'realEstate';
const ASSET_CLASSES: AssetClass[] = [
  'investments',
  'savings',
  'bonds',
  'crypto',
  'otherAssets',
  'realEstate',
];

/** A TEXT amount of a response as a number; unknown counts as nothing. */
const amount = (value: string | null | undefined) => amountOf(value) ?? 0;

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
  const { currencyCode } = useCurrency();
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

  // First UTC day of the selected period (open for "All"). It only changes with
  // the day, so it can key the query; the end is read when the query runs.
  const periodStartSeconds = chartPeriodStart(selectedPeriod, Date.now() / 1000);

  const { data: history } = usePortfolioHistory(periodStartSeconds);
  const {
    data: portfolioMetrics,
    isLoading: metricsLoading,
    isFetching: metricsFetching,
  } = usePortfolioMetrics();

  // Everything below is in the report currency the backend valued it in (ADR 0011)
  const reportCurrency = portfolioMetrics?.currency ?? currencyCode;
  const money = (value: number) => fmt.money(value, reportCurrency, { decimals: 0 });
  const fxQuality = mergeConversionQuality(portfolioMetrics?.fxQuality, history?.fxQuality);

  // Today's values; the last chart point equals the hero number
  const display = {
    netWorth: amount(portfolioMetrics?.netWorth),
    assets: amount(portfolioMetrics?.totalAssets),
    liabilities: amount(portfolioMetrics?.totalLiabilities),
    savings: amount(portfolioMetrics?.totalSavings),
    investments: amount(portfolioMetrics?.totalInvestments),
    bonds: amount(portfolioMetrics?.totalBonds),
    realEstate: amount(portfolioMetrics?.totalRealEstate),
    crypto: amount(portfolioMetrics?.totalCrypto),
    otherAssets: amount(portfolioMetrics?.totalOtherAssets),
  };

  // Brand-new profile: every total is zero. The empty state is only shown once
  // the metrics have settled, so it never flashes during a refetch.
  const allTotalsZero =
    portfolioMetrics !== undefined &&
    [...ASSET_CLASSES.map((cls) => display[cls]), display.assets, display.liabilities].every(
      (value) => value === 0
    );
  const isCheckingPortfolio = metricsLoading || (allTotalsZero && metricsFetching);
  const isEmptyPortfolio = allTotalsZero && !metricsFetching;

  // History in chronological order, every day valued at its own rates by the backend.
  // Personal real estate leaves the history like it leaves today's totals when excluded.
  const historyCurrency = history?.currency;
  const convertedHistory = useMemo(() => {
    // A history still in the previous currency must not meet today's values
    if (historyCurrency !== reportCurrency) return [];
    const excludePersonal = user?.excludePersonalRealEstate || false;
    return (history?.points ?? []).map((h) => {
      const savings = amount(h.totalSavings);
      const investments = amount(h.totalInvestments);
      const bonds = amount(h.totalBonds);
      const crypto = amount(h.totalCrypto);
      const otherAssets = amount(h.totalOtherAssets);
      const realEstate =
        amount(h.totalRealEstateInvestment) +
        (excludePersonal ? 0 : amount(h.totalRealEstatePersonal));
      const liabilities = amount(h.totalLoansPrincipal);
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
  }, [history?.points, historyCurrency, reportCurrency, user?.excludePersonalRealEstate]);

  const oldest = convertedHistory[0];
  // What the change is measured against: the period, or — when the
  // history is shorter than the range — the oldest recorded day.
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
    // display is rebuilt every render; its input is the metrics response
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [oldest, hasHistory, netWorthPct, t, portfolioMetrics]);

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
    value: display[cls],
  }));

  const { moves, isLoading: movesLoading } = useRecentMoves(5);

  const tone = (delta: number) => (delta > 0 ? 'gain' : delta < 0 ? 'loss' : 'neutral');
  const signedMoney = (value: number) =>
    fmt.money(value, reportCurrency, { signed: true, decimals: 0 });

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
            options={CHART_PERIODS}
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
          <div className="skeleton mt-6 h-[220px] w-full" />
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

      {/* Amounts the backend could not convert into the report currency (left out) */}
      <ConversionQualityNote quality={fxQuality} currency={reportCurrency} />

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
            height={220}
            className="mt-[18px] -mx-1"
            formatValue={money}
            renderTip={(p) => ({ title: money(p.value), lines: [fmt.day(p.t)] })}
          />
        }
      >
        <HeroValue
          label={t('hero.label')}
          value={money(display.netWorth)}
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
          value={money(display.assets)}
          note={hasHistory ? `${signedMoney(assetsDelta)} ${changeLabel}` : t('stats.noHistory')}
        />
        <Stat
          label={t('stats.totalLiabilities')}
          aside={<SeriesDot className="bg-s3" />}
          value={money(display.liabilities)}
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
          value={money(display.savings)}
          note={t('stats.freeCashNote')}
        />
      </Stats>

      <UpcomingCard />

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
                size="lg"
                segments={allocation}
                centerLabel={t('allocation.center')}
                centerValue={fmt.number(display.assets, {
                  notation: 'compact',
                  maximumFractionDigits: 2,
                })}
                otherLabel={t('cards.other')}
                formatValue={money}
                formatPercent={(r) => fmt.percent(r, 0)}
              />
            </CardContent>
          </Card>
        </section>
        <section className="flex flex-col">
          <SectionHead title={t('moves.title')} />
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
