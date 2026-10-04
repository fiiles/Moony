import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import type { CurrencyCode } from '@shared/currencies';
import { priceDecimals } from '@shared/currencies';
import { calculatePositionCostBasis } from '@shared/calculations';
import type { DatedConvertFn } from '@shared/calculations';
import { cryptoApi, investmentsApi, type TickerValueHistory } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { nearestIndex, type ChartEvent, type EventCluster } from '@/utils/chart-scale';
import { CHART_PERIODS, chartPeriodStart } from '@/utils/period';
import TimePeriodSelector, { type Period } from '@/components/cashflow/TimePeriodSelector';
import { HeroCard } from '@/components/common/HeroCard';
import { MoonyLineChart } from '@/components/charts/MoonyLineChart';
import { ChartLegend } from '@/components/charts/ChartLegend';

/** A transaction of the position, as the time trace needs it. */
export interface PositionTransaction {
  id: string;
  type: string;
  quantity: string;
  pricePerUnit: string;
  currency: string;
  transactionDate: number;
}

export interface PositionEvent extends ChartEvent {
  tx: PositionTransaction;
}

interface PositionHeroProps {
  type: 'stock' | 'crypto';
  ticker: string;
  /** Current value of the position in the display currency. */
  currentValue: number;
  /** Current cost basis in the display currency (end of the dashed line). */
  currentCost: number;
  transactions: PositionTransaction[];
  convertAt: DatedConvertFn;
  /** The hero numbers on the left (one or more <HeroValue>). */
  children: ReactNode;
  hotEventId?: string | null;
  onHotEventChange?: (id: string | null) => void;
  onEventClick?: (ids: string[]) => void;
  isRefreshing?: boolean;
  /** Unit for the event tooltip ("ks"). */
  unit: string;
}

/**
 * Hero of a single position (design system §7 Detail, §9 time trace): the
 * value line with the dashed "Vloženo" cost basis, buys ▲ and sells ▽ on the
 * line, hover tooltips, highlight synced with the ledger rows, period segment
 * top right.
 */
export function PositionHero({
  type,
  ticker,
  currentValue,
  currentCost,
  transactions,
  convertAt,
  children,
  hotEventId = null,
  onHotEventChange,
  onEventClick,
  isRefreshing = false,
  unit,
}: PositionHeroProps) {
  const { t } = useTranslation('stocks');
  const fmt = useFormat();
  const { convert, currencyCode, formatCurrencyRaw } = useCurrency();
  const queryClient = useQueryClient();
  const [period, setPeriod] = useState<Period>('All');

  useEffect(() => {
    const unlisten = listen('recalculation-complete', () => {
      queryClient.invalidateQueries({ queryKey: ['ticker-history', type, ticker] });
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [queryClient, type, ticker]);

  // On-demand backfill so the chart has history after a fresh import
  useEffect(() => {
    const run = async () => {
      try {
        const result =
          type === 'stock'
            ? await investmentsApi.backfillHistory(ticker)
            : await cryptoApi.backfillHistory(ticker);
        if (result.days_processed > 0) {
          queryClient.invalidateQueries({ queryKey: ['ticker-history', type, ticker] });
        }
      } catch (error) {
        console.error(`[Chart] Backfill failed for ${ticker}:`, error);
      }
    };
    run();
  }, [type, ticker, queryClient]);

  // First UTC day of the selected period (open for "All"). It only changes with
  // the day, so it can key the query; the end is read when the query runs.
  const periodStart = chartPeriodStart(period, Date.now() / 1000);

  const { data: history } = useQuery<TickerValueHistory[]>({
    queryKey: ['ticker-history', type, ticker, periodStart ?? 'all'],
    queryFn: () => {
      const end = Math.floor(Date.now() / 1000);
      return type === 'stock'
        ? investmentsApi.getHistory(ticker, periodStart, end)
        : cryptoApi.getHistory(ticker, periodStart, end);
    },
    staleTime: 0,
    refetchOnMount: 'always',
  });

  // Cost basis after each transaction (chronological), so any day's cost is a lookup
  const costSteps = useMemo(() => {
    const ordered = [...transactions].sort((a, b) => a.transactionDate - b.transactionDate);
    return ordered.map((_, i) => ({
      t: ordered[i].transactionDate,
      value: calculatePositionCostBasis(
        ordered.slice(0, i + 1),
        convertAt,
        currencyCode as CurrencyCode
      ).costBasis,
    }));
  }, [transactions, convertAt, currencyCode]);

  const costAt = (time: number): number => {
    if (costSteps.length === 0) return 0;
    if (time < costSteps[0].t) return 0;
    let i = nearestIndex(costSteps, time);
    if (costSteps[i].t > time) i -= 1;
    return i >= 0 ? costSteps[i].value : 0;
  };

  const points = useMemo(() => {
    const pts = [...(history ?? [])].reverse().map((h) => ({
      t: h.recordedAt,
      value: convert(Number(h.valueCzk) || 0, 'CZK', currencyCode as CurrencyCode),
      cost: costAt(h.recordedAt),
    }));
    const today = utcDayFloor(Date.now() / 1000);
    const last = pts[pts.length - 1];
    if (last && utcDayFloor(last.t) === today) {
      last.value = currentValue;
      last.cost = currentCost;
    } else {
      pts.push({ t: Math.floor(Date.now() / 1000), value: currentValue, cost: currentCost });
    }
    return pts;
    // costAt is derived from costSteps
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [history, convert, currencyCode, currentValue, currentCost, costSteps]);

  const events = useMemo<PositionEvent[]>(
    () =>
      transactions.map((tx) => ({
        id: tx.id,
        t: tx.transactionDate,
        type: tx.type === 'sell' ? 'sell' : 'buy',
        tx,
      })),
    [transactions]
  );

  const eventTip = (cluster: EventCluster<PositionEvent>) => {
    if (cluster.events.length > 1) {
      const first = cluster.events[0].t;
      const last = cluster.events[cluster.events.length - 1].t;
      return {
        title: t('detail.events.cluster', { count: cluster.events.length }),
        lines: [`${fmt.day(first)} – ${fmt.day(last)}`],
      };
    }
    const { tx } = cluster.events[0];
    const qty = Number(tx.quantity) || 0;
    const price = Number(tx.pricePerUnit) || 0;
    const cur = (tx.currency || currencyCode) as CurrencyCode;
    const priceDisplay = convertAt(price, cur, currencyCode as CurrencyCode, tx.transactionDate);
    return {
      title: t(tx.type === 'sell' ? 'detail.events.sell' : 'detail.events.buy', {
        quantity: `${fmt.number(qty, { maximumFractionDigits: 4 })} ${unit}`,
        price: fmt.money(price, cur, { decimals: priceDecimals(price, cur) }),
      }),
      lines: [
        `${fmt.day(tx.transactionDate)} · ${t('detail.events.total', { amount: formatCurrencyRaw(qty * priceDisplay) })}`,
      ],
    };
  };

  return (
    <HeroCard
      className={isRefreshing ? 'opacity-50 transition-opacity duration-base' : undefined}
      aside={<TimePeriodSelector value={period} onChange={setPeriod} options={CHART_PERIODS} />}
      chart={
        <>
          <MoonyLineChart<PositionEvent>
            className="-mx-1 mt-[18px]"
            points={points}
            height={230}
            zeroBaseline={period === 'All'}
            formatValue={(v) => formatCurrencyRaw(v)}
            renderTip={(p) => ({
              title: formatCurrencyRaw(p.value),
              lines: [
                `${fmt.day(p.t)} · ${t('detail.legend.invested')} ${formatCurrencyRaw(p.cost ?? 0)}`,
                `${t('detail.legend.return')} ${fmt.money(p.value - (p.cost ?? 0), currencyCode, { signed: true, decimals: 0 })}`,
              ],
            })}
            events={events}
            renderEventTip={eventTip}
            hotEventId={hotEventId}
            onHotEventChange={onHotEventChange}
            onEventClick={(cluster) => onEventClick?.(cluster.events.map((e) => e.id))}
          />
          <ChartLegend
            items={[
              { label: t('detail.legend.value'), swatch: { kind: 'line' } },
              { label: t('detail.legend.investedLong'), swatch: { kind: 'dash' } },
              { label: t('detail.legend.buy'), swatch: { kind: 'event', type: 'buy' } },
              { label: t('detail.legend.sell'), swatch: { kind: 'event', type: 'sell' } },
            ]}
          />
        </>
      }
    >
      {children}
    </HeroCard>
  );
}
