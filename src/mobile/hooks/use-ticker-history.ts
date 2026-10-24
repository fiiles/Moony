import { useEffect } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import type { TickerValueHistoryReport } from '@shared/schema';
import { useCurrency } from '@/lib/currency';
import { cryptoApi, investmentsApi } from '@/lib/tauri-api';
import { chartPeriodStart, type ChartPeriod } from '@/utils/period';

/**
 * The value and cost line of one ticker for `period`, each day converted into the report
 * currency (the desktop `PositionHero`'s query, so the cache is shared): refetched when the
 * backend finishes a recalculation, and backfilled once per ticker so the chart has history after
 * a fresh import. `kind` picks the stock or the crypto commands; `undefined` until loaded and
 * while there is no ticker.
 */
export function useTickerHistory(
  kind: 'stock' | 'crypto',
  ticker: string | undefined,
  period: ChartPeriod
): TickerValueHistoryReport | undefined {
  const queryClient = useQueryClient();
  const { currencyCode, fxRevision } = useCurrency();
  const api = kind === 'crypto' ? cryptoApi : investmentsApi;

  useEffect(() => {
    if (!ticker) return;
    const unlisten = listen('recalculation-complete', () => {
      queryClient.invalidateQueries({ queryKey: ['ticker-history', kind, ticker] });
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [queryClient, kind, ticker]);

  // On-demand backfill, once per ticker, so the chart has history after a fresh import
  useEffect(() => {
    if (!ticker) return;
    api
      .backfillHistory(ticker)
      .then((result) => {
        if (result.days_processed > 0) {
          queryClient.invalidateQueries({ queryKey: ['ticker-history', kind, ticker] });
        }
      })
      .catch((error) => console.error(`[Chart] Backfill failed for ${ticker}:`, error));
  }, [queryClient, api, kind, ticker]);

  // First UTC day of the period (open for "All"): it only changes with the day, so it keys the query
  const periodStart = chartPeriodStart(period, Date.now() / 1000);
  const { data: history } = useQuery<TickerValueHistoryReport>({
    queryKey: ['ticker-history', kind, ticker, periodStart ?? 'all', currencyCode, fxRevision],
    queryFn: () =>
      api.getHistory(ticker!, currencyCode, periodStart, Math.floor(Date.now() / 1000)),
    enabled: !!ticker,
    staleTime: 0,
    refetchOnMount: 'always',
  });
  return history;
}
