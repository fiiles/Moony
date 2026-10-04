import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import type { PortfolioMetricsHistory } from '@shared/schema';
import { portfolioApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useSyncStatus } from '@/hooks/sync-context';
import {
  useHistoricalDisplayValues,
  type HistoricalDisplayValues,
} from '@/hooks/use-historical-display-values';
import { utcDayFloor } from '@/utils/chart-axis';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import { CHART_PERIODS, chartPeriodStart, type ChartPeriod } from '@/utils/period';
import TimePeriodSelector from '@/components/cashflow/TimePeriodSelector';
import { MoonyLineChart, type LineTip } from '@/components/charts/MoonyLineChart';
import { TREND_CHART_HEIGHT, TrendCard } from '@/components/charts/TrendCard';

type TrendType = 'investments' | 'crypto' | 'realEstate' | 'otherAssets' | 'cash';

interface PortfolioTrendCardProps<E extends ChartEvent> {
  type: TrendType;
  /** Today's value in the display currency; the last point of the line. */
  currentValue: number;
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

/** The part of a history row a trend of `type` shows, in the display currency of that day. */
function historyValue(
  h: PortfolioMetricsHistory,
  type: TrendType,
  convertPoint: HistoricalDisplayValues['convertPoint']
): number {
  switch (type) {
    case 'investments':
      return convertPoint(h.recordedAt, h.investmentsByCurrency, Number(h.totalInvestments));
    case 'crypto':
      return convertPoint(h.recordedAt, h.cryptoByCurrency, Number(h.totalCrypto || 0));
    case 'otherAssets':
      return convertPoint(h.recordedAt, h.otherAssetsByCurrency, Number(h.totalOtherAssets || 0));
    case 'cash':
      return convertPoint(h.recordedAt, h.savingsByCurrency, Number(h.totalSavings || 0));
    case 'realEstate':
      return convertPoint(
        h.recordedAt,
        h.realEstateByCurrency,
        Number(h.totalRealEstatePersonal || 0) + Number(h.totalRealEstateInvestment || 0)
      );
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
  isRefreshing = false,
  earliest,
  events,
  renderEventTip,
  legend,
}: PortfolioTrendCardProps<E>) {
  const { formatCurrencyRaw } = useCurrency();
  const { namespace, group } = TEXTS[type];
  const { t } = useTranslation(namespace);
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const { lastResult } = useSyncStatus();
  const [period, setPeriod] = useState<ChartPeriod>('1Y');

  // Backfill or recalculation finished: the history changed under us
  useEffect(() => {
    if (lastResult && lastResult.days_processed > 0) {
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
    }
  }, [lastResult, queryClient]);

  useEffect(() => {
    const unlisten = listen('recalculation-complete', () => {
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [queryClient]);

  // A whole UTC day, so the query key holds still for the day however often
  // this renders (a start rebuilt from `new Date()` per render never settled)
  const start = chartPeriodStart(period, Date.now() / 1000, earliest);

  const { data: history } = useQuery<PortfolioMetricsHistory[]>({
    queryKey: ['portfolio-history', 'trend', start ?? 'all'],
    queryFn: () => portfolioApi.getHistory(start, Math.floor(Date.now() / 1000)),
    staleTime: 0,
    refetchOnMount: 'always',
  });

  const { convertPoint } = useHistoricalDisplayValues(history);

  const points = useMemo(() => {
    const pts = [...(history ?? [])].reverse().map((h) => ({
      t: h.recordedAt,
      value: historyValue(h, type, convertPoint),
    }));
    const today = utcDayFloor(Date.now() / 1000);
    const last = pts[pts.length - 1];
    if (last && utcDayFloor(last.t) === today) last.value = currentValue;
    else pts.push({ t: Math.floor(Date.now() / 1000), value: currentValue });
    return pts;
  }, [history, convertPoint, type, currentValue]);

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
      legend={legend}
      dimmed={isRefreshing}
    >
      <MoonyLineChart<E>
        points={points}
        height={TREND_CHART_HEIGHT}
        zeroBaseline={period === 'All'}
        formatValue={(v) => formatCurrencyRaw(v)}
        renderTip={(p) => ({ title: formatCurrencyRaw(p.value), lines: [fmt.day(p.t)] })}
        events={visibleEvents}
        renderEventTip={renderEventTip}
      />
    </TrendCard>
  );
}
