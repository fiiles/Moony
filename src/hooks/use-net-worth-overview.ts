import { useMemo } from 'react';
import { useAuth } from '@/hooks/use-auth';
import { useCurrency } from '@/lib/currency';
import { useDatedFacts, usePortfolioHistory, usePortfolioMetrics } from '@/hooks/use-portfolio';
import { amountOf } from '@/utils/positions';
import { bridgeUnknownClasses } from '@/utils/history-gaps';
import {
  detectNetWorthJumps,
  jumpEvents,
  type JumpDay,
  type JumpEvent,
} from '@/utils/net-worth-jumps';
import { mergeConversionQuality } from '@/utils/conversion-quality';
import { utcDayFloor } from '@/utils/chart-axis';
import { chartPeriodStart, type ChartPeriod } from '@/utils/period';
import { shortHistoryBaseDay } from '@/utils/change-base';

export type AssetClass =
  'investments' | 'savings' | 'bonds' | 'crypto' | 'otherAssets' | 'realEstate';
export const ASSET_CLASSES: AssetClass[] = [
  'investments',
  'savings',
  'bonds',
  'crypto',
  'otherAssets',
  'realEstate',
];

/** A TEXT amount of a response as a number; unknown counts as nothing. */
const amount = (value: string | null | undefined) => amountOf(value) ?? 0;

/**
 * Net worth of today and over a chart period, as the overview shows it — shared by the desktop
 * dashboard and the phone's Přehled so both read the same numbers. Everything is in the report
 * currency the backend valued it in (ADR 0011/0012): today's totals, the history of the period
 * with every day at its own rates, the changes since the start of the period, the plotted line
 * with the live value for today and its big steps explained by the dated facts.
 */
export function useNetWorthOverview(period: ChartPeriod) {
  const { user } = useAuth();
  const { currencyCode } = useCurrency();

  // First UTC day of the selected period (open for "All"). It only changes with
  // the day, so it can key the query; the end is read when the query runs.
  const periodStartSeconds = chartPeriodStart(period, Date.now() / 1000);

  const { data: history } = usePortfolioHistory(periodStartSeconds);
  const { data: datedFacts } = useDatedFacts(periodStartSeconds);
  const {
    data: portfolioMetrics,
    isLoading: metricsLoading,
    isFetching: metricsFetching,
  } = usePortfolioMetrics();

  // Everything below is in the report currency the backend valued it in (ADR 0011)
  const reportCurrency = portfolioMetrics?.currency ?? currencyCode;
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
  // A total is null when its classes hold amounts but none converted: shown as "—", never 0
  const unknown = {
    netWorth: portfolioMetrics?.netWorth === null,
    assets: portfolioMetrics?.totalAssets === null,
    liabilities: portfolioMetrics?.totalLiabilities === null,
  };

  // Brand-new profile: every total is zero. The empty state is only shown once
  // the metrics have settled, so it never flashes during a refetch.
  // An unknown (null) total is not zero: it holds amounts that could not be converted.
  const allTotalsZero =
    portfolioMetrics !== undefined &&
    [
      portfolioMetrics.totalSavings,
      portfolioMetrics.totalInvestments,
      portfolioMetrics.totalBonds,
      portfolioMetrics.totalCrypto,
      portfolioMetrics.totalOtherAssets,
      portfolioMetrics.totalRealEstate,
      portfolioMetrics.totalAssets,
      portfolioMetrics.totalLiabilities,
    ].every((value) => amountOf(value) === 0);
  const isCheckingPortfolio = metricsLoading || (allTotalsZero && metricsFetching);
  const isEmptyPortfolio = allTotalsZero && !metricsFetching;

  // History in chronological order, every day valued at its own rates by the backend.
  // Personal real estate leaves the history like it leaves today's totals when excluded.
  // A day lacking (no rate that day) a class today's totals have is left out instead of
  // plotted as a drop by that class.
  const historyCurrency = history?.currency;
  const knownToday = {
    savings: amountOf(portfolioMetrics?.totalSavings) !== null,
    investments: amountOf(portfolioMetrics?.totalInvestments) !== null,
    bonds: amountOf(portfolioMetrics?.totalBonds) !== null,
    crypto: amountOf(portfolioMetrics?.totalCrypto) !== null,
    otherAssets: amountOf(portfolioMetrics?.totalOtherAssets) !== null,
    realEstate: amountOf(portfolioMetrics?.totalRealEstate) !== null,
    liabilities: amountOf(portfolioMetrics?.totalLiabilities) !== null,
  };
  const knownTodayKey = Object.values(knownToday).join();
  const convertedHistory = useMemo(() => {
    // A history still in the previous currency must not meet today's values
    if (historyCurrency !== reportCurrency) return [];
    const excludePersonal = user?.excludePersonalRealEstate || false;
    return (history?.points ?? []).flatMap((h) => {
      const investmentRealEstate = amountOf(h.totalRealEstateInvestment);
      const personalRealEstate = excludePersonal ? 0 : amountOf(h.totalRealEstatePersonal);
      const day = bridgeUnknownClasses(
        {
          savings: amountOf(h.totalSavings),
          investments: amountOf(h.totalInvestments),
          bonds: amountOf(h.totalBonds),
          crypto: amountOf(h.totalCrypto),
          otherAssets: amountOf(h.totalOtherAssets),
          realEstate:
            investmentRealEstate === null || personalRealEstate === null
              ? null
              : investmentRealEstate + personalRealEstate,
          liabilities: amountOf(h.totalLoansPrincipal),
        },
        knownToday
      );
      if (day === null) return [];
      const assets =
        day.savings + day.investments + day.bonds + day.realEstate + day.crypto + day.otherAssets;
      return [
        {
          recordedAt: h.recordedAt,
          ...day,
          assets,
          netWorth: assets - day.liabilities,
        },
      ];
    });
    // knownToday is rebuilt every render; knownTodayKey carries its content
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    history?.points,
    historyCurrency,
    reportCurrency,
    user?.excludePersonalRealEstate,
    knownTodayKey,
  ]);

  const oldest = convertedHistory[0];
  // What the change is measured against: the period, or — when the
  // history is shorter than the range — the oldest recorded day.
  const shortHistoryBase = shortHistoryBaseDay(periodStartSeconds, oldest?.recordedAt);
  const hasHistory = convertedHistory.length > 1;
  const netWorthDelta = oldest ? display.netWorth - oldest.netWorth : 0;
  const netWorthPct =
    oldest && oldest.netWorth !== 0 ? netWorthDelta / Math.abs(oldest.netWorth) : 0;
  const assetsDelta = oldest ? display.assets - oldest.assets : 0;
  const liabilitiesDelta = oldest ? display.liabilities - oldest.liabilities : 0;

  // Hero chart: history plus the live value for today (UTC days, ADR 0008). Every plotted day
  // carries what each class contributes, so a big step can be traced to the classes behind it.
  const plotted = useMemo<JumpDay[]>(() => {
    const todayStart = utcDayFloor(Date.now() / 1000);
    const dayOf = (
      t: number,
      netWorth: number,
      c: Record<AssetClass | 'liabilities', number>
    ): JumpDay => ({
      t,
      netWorth,
      classes: {
        savings: c.savings,
        investments: c.investments,
        crypto: c.crypto,
        bonds: c.bonds,
        realEstate: c.realEstate,
        loans: -c.liabilities,
        otherAssets: c.otherAssets,
      },
    });
    const days = convertedHistory.map((h) => dayOf(h.recordedAt, h.netWorth, h));
    // An unknown net worth today is left out, not plotted as a drop to 0
    if (unknown.netWorth) return days.filter((d) => utcDayFloor(d.t) !== todayStart);
    const last = days[days.length - 1];
    if (last && utcDayFloor(last.t) === todayStart) {
      days[days.length - 1] = dayOf(last.t, display.netWorth, display);
    } else {
      days.push(dayOf(Math.floor(Date.now() / 1000), display.netWorth, display));
    }
    return days;
    // display is rebuilt every render; its input is the metrics response
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [convertedHistory, portfolioMetrics, unknown.netWorth]);
  const points = useMemo(() => plotted.map((d) => ({ t: d.t, value: d.netWorth })), [plotted]);

  // Big steps of the plotted line, explained by the dated facts of their day. Until the facts
  // are in, nothing is marked: without them a purchase day would read as an unexplained step.
  const jumps = useMemo<JumpEvent[]>(
    () => (datedFacts === undefined ? [] : jumpEvents(detectNetWorthJumps(plotted, datedFacts))),
    [plotted, datedFacts]
  );

  return {
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
  };
}
