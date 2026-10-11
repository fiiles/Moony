import { keepPreviousData, useQuery } from '@tanstack/react-query';
import type { StockBenchmark, StockPerformance, StockPerformanceRequest } from '@shared/schema';
import { investmentsApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';

/**
 * Performance of the held positions for one page state (period, report currency, the series to
 * chart), as `get_stock_performance` returns it. The rate revision is part of the key, so the
 * returns refetch when the rates change; investment mutations, price refreshes and tag edits
 * invalidate the `['stock-performance']` prefix. Pass `null` while there is nothing to request.
 *
 * The previous response stays in `data` while the next one loads (`isPlaceholderData`), so the
 * chart and the board do not blink when the period or the selection changes.
 */
export function useStockPerformance(request: StockPerformanceRequest | null) {
  const { fxRevision } = useCurrency();
  const query = useQuery<StockPerformance>({
    queryKey: ['stock-performance', request, fxRevision],
    queryFn: () => {
      if (!request) throw new Error('performance request missing');
      return investmentsApi.getStockPerformance(request);
    },
    enabled: request !== null,
    placeholderData: keepPreviousData,
  });
  return {
    data: query.data,
    isLoading: query.isLoading,
    isPlaceholderData: query.isPlaceholderData,
    isError: query.isError,
    refetch: query.refetch,
  };
}

/** How long a loaded index stays fresh; a failed load (no dates) is refetched when asked again. */
const BENCHMARK_STALE_MS = 5 * 60_000;

export interface BenchmarkRequest {
  ticker: string;
  /** First and last day (unix seconds, UTC midnight) of the performance on screen. */
  fromTs: number;
  toTs: number;
  reportCurrency: string;
}

/**
 * Daily returns of an index over the period on screen, fetched from Yahoo on demand and kept only
 * in the query cache (never stored). Pass `null` while no index is chosen. Offline or for an
 * unknown ticker the response has no `dates`; a thrown error means the same to the page.
 */
export function useStockBenchmark(request: BenchmarkRequest | null) {
  const { fxRevision } = useCurrency();
  const query = useQuery<StockBenchmark>({
    queryKey: [
      'stock-benchmark',
      request?.ticker,
      request?.fromTs,
      request?.toTs,
      request?.reportCurrency,
      fxRevision,
    ],
    queryFn: () => {
      if (!request) throw new Error('benchmark request missing');
      return investmentsApi.getStockBenchmark(
        request.ticker,
        request.fromTs,
        request.toTs,
        request.reportCurrency
      );
    },
    enabled: request !== null,
    staleTime: (q) => (q.state.data && q.state.data.dates.length === 0 ? 0 : BENCHMARK_STALE_MS),
  });
  return {
    data: query.data,
    isLoading: query.isLoading,
    isError: query.isError,
  };
}
