import { useEffect } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import type { DatedFact, PortfolioHistoryResponse, PortfolioMetrics } from '@shared/schema';
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
 * still; mutations invalidate the `['portfolio-history']` prefix. The backend re-derives the
 * days in the background after a write (a balance record, a valuation, a transaction, …) and
 * fires `recalculation-complete` when a batch is done, so the history refetches then.
 */
export function usePortfolioHistory(start: number | undefined) {
  const { currencyCode, fxRevision } = useCurrency();
  const queryClient = useQueryClient();

  useEffect(() => {
    const unlisten = listen('recalculation-complete', () => {
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [queryClient]);

  return useQuery<PortfolioHistoryResponse>({
    queryKey: ['portfolio-history', currencyCode, fxRevision, start ?? 'all'],
    queryFn: () => portfolioApi.getHistory(currencyCode, start, Math.floor(Date.now() / 1000)),
    staleTime: 0,
    refetchOnMount: 'always',
  });
}

/**
 * Dated facts (purchases, estimates, new loans, manual balances, days of trades) from `start`
 * (unix seconds, open for everything) until now: what the dashboard names behind a big step of
 * net worth. The facts are native and need no rates, so the key holds no currency. It sits under
 * the `['portfolio-history']` prefix on purpose: every write that re-derives the history (a
 * mutation, a price refresh, a finished `recalculation-complete`, the MCP data events) invalidates
 * that prefix, and the facts those writes add or move are refetched with it.
 */
export function useDatedFacts(start: number | undefined) {
  return useQuery<DatedFact[]>({
    queryKey: ['portfolio-history', 'dated-facts', start ?? 'all'],
    queryFn: () => portfolioApi.getDatedFacts(start, Math.floor(Date.now() / 1000)),
    staleTime: 0,
    refetchOnMount: 'always',
  });
}
