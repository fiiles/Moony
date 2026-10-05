import { useQuery, useQueryClient } from '@tanstack/react-query';
import { stockMonitorApi } from '@/lib/tauri-api';
import type { ChartPeriod } from '@/utils/stock-monitor';

/** Poll cadence (spec D3). TanStack gates each interval tick on
 *  focusManager.isFocused() (fresh visibilityState read), so polling pauses
 *  while hidden and self-resumes — no custom gating needed. The backend
 *  refresh is TTL-guarded at 15 min, so 5-min polls between refreshes are
 *  cheap no-ops. */
const POLL_INTERVAL_MS = 5 * 60 * 1000;

/** Mirrors the Investments page: a short window where cached rows render
 *  instantly on remount instead of re-querying. */
const ROWS_STALE_TIME_MS = 60 * 1000;

export function useWatchedStocks() {
  const queryClient = useQueryClient();
  return useQuery({
    queryKey: ['stock-monitor'],
    queryFn: async () => {
      // Read first and return immediately: awaiting the Yahoo refresh here
      // would block the whole table (and its logos) behind a network call on
      // every mount and every mutation. The Investments page reads cheaply for
      // the same reason; the refresh runs in the background instead.
      const rows = await stockMonitorApi.getWatched();
      void stockMonitorApi
        .refreshPrices()
        .then((result) => {
          // Re-read only when prices actually moved. A follow-up refresh is a
          // TTL no-op (empty `updated`), so this cannot loop.
          if (result.updated.length > 0) {
            queryClient.invalidateQueries({ queryKey: ['stock-monitor'] });
            // A price that moved may have crossed a target: the overview card and the
            // top-bar indicator should not wait for their own refresh.
            queryClient.invalidateQueries({ queryKey: ['milestones'] });
          }
        })
        .catch((error) => {
          console.error('[stock-monitor] background price refresh failed:', error);
        });
      return rows;
    },
    refetchOnMount: true,
    staleTime: ROWS_STALE_TIME_MS,
    refetchInterval: POLL_INTERVAL_MS,
  });
}

/** Portfolio stocks not followed yet — empty means the button stays hidden. */
export function usePortfolioFollowCandidates() {
  return useQuery({
    queryKey: ['stock-monitor', 'portfolio-candidates'],
    queryFn: () => stockMonitorApi.getPortfolioCandidates(),
    staleTime: ROWS_STALE_TIME_MS,
  });
}

export function useStockMonitorDetail(ticker: string | undefined) {
  return useQuery({
    queryKey: ['stock-monitor', 'detail', ticker],
    queryFn: () => stockMonitorApi.getDetail(ticker!),
    enabled: !!ticker,
  });
}

export function useStockPriceRange(ticker: string | undefined, period: ChartPeriod) {
  return useQuery({
    queryKey: ['stock-monitor', 'range', ticker, period],
    queryFn: () => stockMonitorApi.getPriceRange(ticker!, period),
    enabled: !!ticker,
    staleTime: 5 * 60 * 1000,
  });
}
