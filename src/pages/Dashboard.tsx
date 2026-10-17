import { useMemo, useState } from 'react';
import { useLocation } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Landmark, Plus, Target, Wallet } from 'lucide-react';
import { useFormat } from '@/lib/use-format';
import { useAuth } from '@/hooks/use-auth';
import { useGreeting } from '@/hooks/use-greeting';
import {
  ASSET_CLASSES,
  type AssetClass,
  useNetWorthOverview,
} from '@/hooks/use-net-worth-overview';
import { useNetWorthJumpTip } from '@/hooks/use-net-worth-jump-tip';
import { useRecentMoves } from '@/hooks/use-recent-moves';
import type { JumpEvent } from '@/utils/net-worth-jumps';
import { investmentPage } from '@/utils/getting-started';
import { CHART_PERIODS } from '@/utils/period';
import TimePeriodSelector, { type Period } from '@/components/cashflow/TimePeriodSelector';
import { PageHead, SectionHead } from '@/components/shell/PageHead';
import { HeroCard, HeroValue } from '@/components/common/HeroCard';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend } from '@/components/charts/ChartLegend';
import { SeriesDot, Stat, StatSkeleton, Stats } from '@/components/common/Stat';
import { EmptyState } from '@/components/common/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { GettingStartedCard } from '@/components/dashboard/GettingStartedCard';
import { AllocationRing } from '@/components/dashboard/AllocationRing';
import { RecentMoves } from '@/components/dashboard/RecentMoves';
import { UpcomingCard } from '@/components/dashboard/UpcomingCard';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';

export default function Dashboard() {
  const { t } = useTranslation('dashboard');
  const [, setLocation] = useLocation();
  const { user } = useAuth();
  const fmt = useFormat();
  const [selectedPeriod, setSelectedPeriod] = useState<Period>('30D');

  const greeting = useGreeting();
  const asOf = t('asOf', {
    date: fmt.date(new Date(), { day: 'numeric', month: 'long', year: 'numeric' }),
  });

  const {
    portfolioMetrics,
    reportCurrency,
    fxQuality,
    display,
    unknown,
    isCheckingPortfolio,
    isEmptyPortfolio,
    oldest,
    shortHistoryBase,
    hasHistory,
    netWorthDelta,
    netWorthPct,
    assetsDelta,
    liabilitiesDelta,
    points,
    jumps,
  } = useNetWorthOverview(selectedPeriod);

  // Everything below is in the report currency the backend valued it in (ADR 0011)
  const money = (value: number) => fmt.money(value, reportCurrency, { decimals: 0 });
  const totalMoney = (value: number, isUnknown: boolean) => (isUnknown ? '—' : money(value));

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

  // Insight: the asset class with the largest absolute contribution to the change
  const insight = useMemo(() => {
    if (!oldest || !hasHistory) return t('insight.noHistory');
    // An unknown net worth today has no change to explain
    if (unknown.netWorth) return null;
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
  }, [oldest, hasHistory, netWorthPct, t, portfolioMetrics, unknown.netWorth]);

  const jumpTip = useNetWorthJumpTip(reportCurrency);

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
    // Only the areas chosen in the wizard / Settings get a shortcut
    const investmentHref = investmentPage(user?.menuPreferences);
    return (
      <>
        {head}
        <GettingStartedCard />
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
              {investmentHref && (
                <Button variant="outline" onClick={() => setLocation(investmentHref)}>
                  <Plus />
                  {t('empty.addInvestment')}
                </Button>
              )}
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
          <>
            <MoonyLineChart<JumpEvent>
              points={points}
              zeroBaseline={selectedPeriod === 'All'}
              height={220}
              className="mt-[18px] -mx-1"
              formatValue={money}
              renderTip={(p) => ({ title: money(p.value), lines: [fmt.day(p.t)] })}
              events={jumps}
              renderEventTip={jumpTip}
            />
            {jumps.length > 0 && (
              <ChartLegend
                items={[{ label: t('jumps.legend'), swatch: { kind: 'event', type: 'mark' } }]}
              />
            )}
          </>
        }
      >
        <HeroValue
          label={t('hero.label')}
          value={totalMoney(display.netWorth, unknown.netWorth)}
          tone={tone(netWorthDelta)}
          delta={
            hasHistory && !unknown.netWorth
              ? `${netWorthDelta >= 0 ? '↗' : '↘'} ${fmt.percent(netWorthPct, 1, { signed: true })}`
              : undefined
          }
          deltaNote={
            unknown.netWorth
              ? undefined
              : hasHistory
                ? `${signedMoney(netWorthDelta)} ${changeLabel}`
                : t('stats.noHistory')
          }
        />
      </HeroCard>

      <Stats columns={3} className="mt-[15px]">
        <Stat
          label={t('stats.totalAssets')}
          aside={<SeriesDot className="bg-s1" />}
          value={totalMoney(display.assets, unknown.assets)}
          note={
            unknown.assets
              ? undefined
              : hasHistory
                ? `${signedMoney(assetsDelta)} ${changeLabel}`
                : t('stats.noHistory')
          }
        />
        <Stat
          label={t('stats.totalLiabilities')}
          aside={<SeriesDot className="bg-s3" />}
          value={totalMoney(display.liabilities, unknown.liabilities)}
          note={
            unknown.liabilities
              ? undefined
              : !hasHistory
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
