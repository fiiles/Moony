import { useQuery } from '@tanstack/react-query';
import type { PortfolioHistoryResponse, PortfolioMetrics } from '@shared/schema';
import { portfolioApi } from '@/lib/tauri-api';
import { useAuth } from '@/hooks/use-auth';
import { useCurrency } from '@/lib/currency';

/**
 * Current net worth in the profile currency, valued by the backend (ADR 0011). The quote
 * revision is part of the key, so the values refetch when the rates change; mutations
 * invalidate the `['portfolio-metrics']` prefix.
 */
export function usePortfolioMetrics() {
  const { user } = useAuth();
  const { currencyCode, fxRevision } = useCurrency();
  const excludePersonal = user?.excludePersonalRealEstate ?? false;
  return useQuery<PortfolioMetrics>({
    queryKey: ['portfolio-metrics', currencyCode, fxRevision, excludePersonal],
    queryFn: () => portfolioApi.getMetrics(currencyCode, excludePersonal),
    staleTime: 0,
    refetchOnMount: 'always',
  });
}

/**
 * Recorded portfolio days from `start` (unix seconds, open for everything) until now, in the
 * profile currency, every day at its own rates. Keep `start` a whole UTC day so the key holds
 * still; mutations invalidate the `['portfolio-history']` prefix.
 */
export function usePortfolioHistory(start: number | undefined) {
  const { currencyCode, fxRevision } = useCurrency();
  return useQuery<PortfolioHistoryResponse>({
    queryKey: ['portfolio-history', currencyCode, fxRevision, start ?? 'all'],
    queryFn: () => portfolioApi.getHistory(currencyCode, start, Math.floor(Date.now() / 1000)),
    staleTime: 0,
    refetchOnMount: 'always',
  });
}
