import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { PortfolioHistoryPoint } from '@shared/schema';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { usePortfolioHistory } from '@/hooks/use-portfolio';
import { utcDayFloor } from '@/utils/chart-axis';
import { amountOf } from '@/utils/positions';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import { CHART_PERIODS, chartPeriodStart, type ChartPeriod } from '@/utils/period';
import TimePeriodSelector from '@/components/cashflow/TimePeriodSelector';
import { MoonyLineChart, type LineTip } from '@/components/charts/MoonyLineChart';
import { TREND_CHART_HEIGHT, TrendCard } from '@/components/charts/TrendCard';
import { ConversionQualityNote } from '@/components/reports/ReportQualityNote';

type TrendType = 'investments' | 'crypto' | 'realEstate' | 'otherAssets' | 'cash';

interface PortfolioTrendCardProps<E extends ChartEvent> {
  type: TrendType;
  /**
   * Today's value in the profile (report) currency; the last point of the line. `null` when
   * unknown (held, none converted): today is left out instead of plotted as 0.
   */
  currentValue: number | null;
  /**
   * Currencies today's value leaves out for want of a rate (named in the note with the
   * history's own misses).
   */
  missingCurrencies?: string[];
  isRefreshing?: boolean;
  /** First day (UTC, unix seconds) with data; no horizon starts before it. */
  earliest?: number;
  /** Optional time-trace events on the aggregate line (stocks, crypto: buy and sell days). */
  events?: E[];
  renderEventTip?: (cluster: EventCluster<E>) => LineTip;
  /** Legend under the chart when the card carries events. */
  legend?: ReactNode;
}

/** Where each type finds its heading: the keys `<group>.title` and `<group>.subtitle`. */
const TEXTS: Record<TrendType, { namespace: string; group: string }> = {
  investments: { namespace: 'stocks', group: 'chart' },
  crypto: { namespace: 'crypto', group: 'chart' },
  realEstate: { namespace: 'realEstate', group: 'chart' },
  otherAssets: { namespace: 'otherAssets', group: 'chart' },
  cash: { namespace: 'bank_accounts', group: 'trend' },
};

/**
 * The part of a history day a trend of `type` shows (report currency); null when nothing in
 * it could be converted that day.
 */
function historyValue(h: PortfolioHistoryPoint, type: TrendType): number | null {
  const amount = (value: string | null) => amountOf(value);
  switch (type) {
    case 'investments':
      return amount(h.totalInvestments);
    case 'crypto':
      return amount(h.totalCrypto);
    case 'otherAssets':
      return amount(h.totalOtherAssets);
    case 'cash':
      return amount(h.totalSavings);
    case 'realEstate': {
      const personal = amount(h.totalRealEstatePersonal);
      const investment = amount(h.totalRealEstateInvestment);
      return personal === null || investment === null ? null : personal + investment;
    }
  }
}

/**
 * Compact trend of a whole asset class (design system §7 List: "optional
 * trend card") on the shared `TrendCard`: title, one sentence, the five
 * horizons and a 170 px value line from the recorded portfolio history. The
 * stocks and crypto lists add their buy and sell days as events; the bank
 * accounts list shows the recorded cash (every balance change is snapshotted,
 * so a hand-kept account appears as steps and the line ends at the cash in
 * net worth).
 */
export default function PortfolioTrendCard<E extends ChartEvent = ChartEvent>({
  type,
  currentValue,
  missingCurrencies,
  isRefreshing = false,
  earliest,
  events,
  renderEventTip,
  legend,
}: PortfolioTrendCardProps<E>) {
  const { currencyCode } = useCurrency();
  const { namespace, group } = TEXTS[type];
  const { t } = useTranslation(namespace);
  const fmt = useFormat();
  const [period, setPeriod] = useState<ChartPeriod>('1Y');

  // The history refetches when a backend rebuild finishes: `usePortfolioHistory` listens.
  // A whole UTC day, so the query key holds still for the day however often
  // this renders (a start rebuilt from `new Date()` per render never settled)
  const start = chartPeriodStart(period, Date.now() / 1000, earliest);

  // History in the report currency, every day at its own rates (ADR 0011)
  const { data: history } = usePortfolioHistory(start);
  const money = (value: number) => fmt.money(value, currencyCode, { decimals: 0 });
  // Amounts that could not be converted: the history's days and today's value leave them out
  const missing = useMemo(
    () => [
      ...new Set([...(history?.fxQuality.missingCurrencies ?? []), ...(missingCurrencies ?? [])]),
    ],
    [history, missingCurrencies]
  );
  const incomplete = missing.length > 0;

  const points = useMemo(() => {
    // A history still in the previous currency must not meet today's value
    const days = history?.currency === currencyCode ? history.points : [];
    // A day whose class could not be converted at all is left out, not plotted as a drop to 0
    const pts = days.flatMap((h) => {
      const value = historyValue(h, type);
      return value === null ? [] : [{ t: h.recordedAt, value }];
    });
    const today = utcDayFloor(Date.now() / 1000);
    if (currentValue === null) return pts.filter((p) => utcDayFloor(p.t) !== today);
    const last = pts[pts.length - 1];
    if (last && utcDayFloor(last.t) === today) last.value = currentValue;
    else pts.push({ t: Math.floor(Date.now() / 1000), value: currentValue });
    return pts;
  }, [history, currencyCode, type, currentValue]);

  // Events before the horizon would pile up on the left edge
  const visibleEvents = useMemo(
    () => (start === undefined ? events : events?.filter((e) => e.t >= start)),
    [events, start]
  );

  return (
    <TrendCard
      title={t(`${group}.title`)}
      subtitle={t(`${group}.subtitle`)}
      aside={<TimePeriodSelector value={period} onChange={setPeriod} options={CHART_PERIODS} />}
      legend={
        legend || incomplete ? (
          <>
            {legend}
            <ConversionQualityNote
              quality={{ missingCurrencies: missing }}
              currency={currencyCode}
              className="mb-0 mt-2"
            />
          </>
        ) : undefined
      }
      dimmed={isRefreshing}
    >
      <MoonyLineChart<E>
        points={points}
        height={TREND_CHART_HEIGHT}
        zeroBaseline={period === 'All'}
        formatValue={money}
        renderTip={(p) => ({ title: money(p.value), lines: [fmt.day(p.t)] })}
        events={visibleEvents}
        renderEventTip={renderEventTip}
      />
    </TrendCard>
  );
}
