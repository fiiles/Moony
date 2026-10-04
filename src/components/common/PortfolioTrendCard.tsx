import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { subDays, startOfYear } from 'date-fns';
import { listen } from '@tauri-apps/api/event';
import type { PortfolioMetricsHistory } from '@shared/schema';
import { portfolioApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useSyncStatus } from '@/hooks/sync-context';
import { useHistoricalDisplayValues } from '@/hooks/use-historical-display-values';
import { utcDayFloor } from '@/utils/chart-axis';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
import TimePeriodSelector, { type Period } from '@/components/cashflow/TimePeriodSelector';
import { Card } from '@/components/ui/card';
import { MoonyLineChart, type LineTip } from '@/components/charts/MoonyLineChart';
import { cn } from '@/lib/utils';

/** Transactions passed by the list pages; only the earliest date bounds "Vše". */
export interface TransactionMarker {
  date: number;
  buyAmount: number;
  sellAmount: number;
  buyTickers?: string[];
  sellTickers?: string[];
}

interface PortfolioTrendCardProps<E extends ChartEvent> {
  type: 'investments' | 'crypto' | 'realEstate' | 'otherAssets';
  currentValue: number;
  isRefreshing?: boolean;
  transactionMarkers?: TransactionMarker[];
  /** Optional time-trace events on the aggregate line (crypto: buy and sell days). */
  events?: E[];
  renderEventTip?: (cluster: EventCluster<E>) => LineTip;
  /** Legend under the chart when the card carries events. */
  legend?: ReactNode;
}

const PERIODS: readonly Period[] = ['30D', '90D', '1Y', 'All'];

/**
 * Compact trend of a whole asset class (design system §7 List: "optional
 * trend card"): title, one sentence, period segment, 96 px value line. The
 * crypto list adds its buy and sell days as events (approved 2026-10-03).
 */
export default function PortfolioTrendCard<E extends ChartEvent = ChartEvent>({
  type,
  currentValue,
  isRefreshing = false,
  transactionMarkers = [],
  events,
  renderEventTip,
  legend,
}: PortfolioTrendCardProps<E>) {
  const { formatCurrencyRaw } = useCurrency();
  const namespace = {
    investments: 'stocks',
    crypto: 'crypto',
    realEstate: 'realEstate',
    otherAssets: 'otherAssets',
  }[type];
  const { t } = useTranslation(namespace);
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const { lastResult } = useSyncStatus();
  const [selectedPeriod, setSelectedPeriod] = useState<Period>('1Y');

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

  const dateRange = useMemo(() => {
    const now = new Date();
    const earliest =
      transactionMarkers.length > 0
        ? new Date(Math.min(...transactionMarkers.map((m) => m.date)) * 1000)
        : undefined;
    let start: Date;
    switch (selectedPeriod) {
      case '30D':
        start = subDays(now, 30);
        break;
      case '90D':
        start = subDays(now, 90);
        break;
      case 'YTD':
        start = startOfYear(now);
        break;
      case '1Y':
        start = subDays(now, 365);
        break;
      case '5Y':
        start = subDays(now, 365 * 5);
        break;
      default:
        start = earliest ?? subDays(now, 365 * 5);
    }
    // Never show days before the first transaction
    const effective = earliest && start < earliest ? earliest : start;
    return { start: effective, end: now };
  }, [selectedPeriod, transactionMarkers]);

  const { data: history } = useQuery<PortfolioMetricsHistory[]>({
    queryKey: ['portfolio-history', dateRange.start.toISOString(), dateRange.end.toISOString()],
    queryFn: () =>
      portfolioApi.getHistory(
        Math.floor(dateRange.start.getTime() / 1000),
        Math.floor(dateRange.end.getTime() / 1000)
      ),
    staleTime: 0,
    refetchOnMount: 'always',
  });

  const { convertPoint } = useHistoricalDisplayValues(history);

  const points = useMemo(() => {
    const pts = [...(history ?? [])].reverse().map((h) => ({
      t: h.recordedAt,
      value:
        type === 'investments'
          ? convertPoint(h.recordedAt, h.investmentsByCurrency, Number(h.totalInvestments))
          : type === 'crypto'
            ? convertPoint(h.recordedAt, h.cryptoByCurrency, Number(h.totalCrypto || 0))
            : type === 'otherAssets'
              ? convertPoint(h.recordedAt, h.otherAssetsByCurrency, Number(h.totalOtherAssets || 0))
              : convertPoint(
                  h.recordedAt,
                  h.realEstateByCurrency,
                  Number(h.totalRealEstatePersonal || 0) + Number(h.totalRealEstateInvestment || 0)
                ),
    }));
    const today = utcDayFloor(Date.now() / 1000);
    const last = pts[pts.length - 1];
    if (last && utcDayFloor(last.t) === today) last.value = currentValue;
    else pts.push({ t: Math.floor(Date.now() / 1000), value: currentValue });
    return pts;
  }, [history, convertPoint, type, currentValue]);

  // Events outside the visible range would pile up on the left edge
  const startSec = Math.floor(dateRange.start.getTime() / 1000);
  const visibleEvents = useMemo(() => events?.filter((e) => e.t >= startSec), [events, startSec]);

  return (
    <Card
      className={cn(
        'relative mb-7 overflow-hidden px-[21px] pb-[14px] pt-5 transition-opacity duration-base after:pointer-events-none after:absolute after:-right-[100px] after:-top-[120px] after:h-[180px] after:w-[340px] after:rounded-full after:bg-hero-orb after:content-[""]',
        isRefreshing && 'opacity-50'
      )}
    >
      <div className="relative z-[1] flex items-center justify-between gap-6">
        <div>
          <h3 className="m-0 text-h3 text-ink">{t('chart.title')}</h3>
          <p className="mt-[5px] text-micro font-500 text-ink-4">{t('chart.subtitle')}</p>
        </div>
        <TimePeriodSelector value={selectedPeriod} onChange={setSelectedPeriod} options={PERIODS} />
      </div>
      <MoonyLineChart<E>
        className="relative z-[1] -mx-1 mt-[14px]"
        points={points}
        height={events ? 110 : 96}
        zeroBaseline={selectedPeriod === 'All'}
        formatValue={(v) => formatCurrencyRaw(v)}
        renderTip={(p) => ({ title: formatCurrencyRaw(p.value), lines: [fmt.day(p.t)] })}
        events={visibleEvents}
        renderEventTip={renderEventTip}
      />
      {legend && <div className="relative z-[1]">{legend}</div>}
    </Card>
  );
}
