import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import type { CurrencyCode } from '@shared/currencies';
import { priceDecimals } from '@shared/currencies';
import type { TickerValueHistoryReport } from '@shared/schema';
import { cryptoApi, investmentsApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { amountOf } from '@/utils/positions';
import { utcDayFloor } from '@/utils/chart-axis';
import type { ChartEvent, EventCluster } from '@/utils/chart-scale';
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
  /** Current value of the position in the report currency; `null` when not convertible
   *  (the line then ends at the last recorded day). */
  currentValue: number | null;
  /** The position's cost basis in the report currency, as the backend states it (the dashed
   *  line, from the first transaction on); `null` when unknown (no dashed line). */
  currentCost: number | null;
  transactions: PositionTransaction[];
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
  children,
  hotEventId = null,
  onHotEventChange,
  onEventClick,
  isRefreshing = false,
  unit,
}: PositionHeroProps) {
  const { t } = useTranslation('stocks');
  const fmt = useFormat();
  const { currencyCode, fxRevision } = useCurrency();
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

  // The backend converts every day's value into the report currency at that day's rate
  const { data: history } = useQuery<TickerValueHistoryReport>({
    queryKey: ['ticker-history', type, ticker, periodStart ?? 'all', currencyCode, fxRevision],
    queryFn: () => {
      const end = Math.floor(Date.now() / 1000);
      return type === 'stock'
        ? investmentsApi.getHistory(ticker, currencyCode, periodStart, end)
        : cryptoApi.getHistory(ticker, currencyCode, periodStart, end);
    },
    staleTime: 0,
    refetchOnMount: 'always',
  });
  const reportCurrency = history?.currency ?? currencyCode;
  const money = (v: number) => fmt.money(v, reportCurrency, { decimals: 0 });

  const firstTransaction = useMemo(
    () =>
      transactions.reduce<number | null>(
        (min, tx) => (min === null || tx.transactionDate < min ? tx.transactionDate : min),
        null
      ),
    [transactions]
  );
  const costAt = (time: number): number | undefined =>
    currentCost !== null && firstTransaction !== null && time >= firstTransaction
      ? currentCost
      : undefined;

  const points = useMemo(() => {
    const pts = (history?.points ?? []).flatMap((h) => {
      const value = amountOf(h.value);
      return value === null ? [] : [{ t: h.recordedAt, value, cost: costAt(h.recordedAt) }];
    });
    if (currentValue === null) return pts;
    const today = utcDayFloor(Date.now() / 1000);
    const last = pts[pts.length - 1];
    const now = Math.floor(Date.now() / 1000);
    if (last && utcDayFloor(last.t) === today) {
      last.value = currentValue;
      last.cost = costAt(now);
    } else {
      pts.push({ t: now, value: currentValue, cost: costAt(now) });
    }
    return pts;
    // costAt is derived from currentCost and the first transaction
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [history, currentValue, currentCost, firstTransaction]);

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
    return {
      title: t(tx.type === 'sell' ? 'detail.events.sell' : 'detail.events.buy', {
        quantity: `${fmt.number(qty, { maximumFractionDigits: 4 })} ${unit}`,
        price: fmt.money(price, cur, { decimals: priceDecimals(price, cur) }),
      }),
      lines: [
        `${fmt.day(tx.transactionDate)} · ${t('detail.events.total', { amount: fmt.money(qty * price, cur) })}`,
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
            formatValue={money}
            renderTip={(p) => ({
              title: money(p.value),
              lines:
                p.cost === undefined
                  ? [fmt.day(p.t)]
                  : [
                      `${fmt.day(p.t)} · ${t('detail.legend.invested')} ${money(p.cost)}`,
                      `${t('detail.legend.return')} ${fmt.money(p.value - p.cost, reportCurrency, { signed: true, decimals: 0 })}`,
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
