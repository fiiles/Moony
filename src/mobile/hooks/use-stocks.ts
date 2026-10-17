import { useMemo } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { InvestmentTransaction, StockPositions } from '@shared/schema';
import { useTransactionFx } from '@/hooks/use-fx';
import { useCurrency } from '@/lib/currency';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import { investmentsApi, priceApi } from '@/lib/tauri-api';
import { useStockPerformance } from '@/hooks/use-stock-performance';
import { toStockRow } from '@/utils/positions';
import { ytdRange } from '@/mobile/lib/position-series';

/**
 * The positions as the backend values them in the report currency (ADR 0011): the query of the
 * desktop's Akcie page and of Majetek (`['investments', currency, revision]`), so the cache is
 * shared. `rows` are the positions parsed into numbers; `positions` carries the totals and the
 * conversion quality.
 */
export function useStockPositions() {
  const { currencyCode, fxRevision } = useCurrency();
  const { data: positions, isLoading } = useQuery<StockPositions>({
    queryKey: ['investments', currencyCode, fxRevision],
    queryFn: () => investmentsApi.getAll(currencyCode),
    refetchOnMount: true,
    staleTime: 60 * 1000,
  });
  const rows = useMemo(() => (positions?.items ?? []).map(toStockRow), [positions]);
  return { positions, rows, reportCurrency: positions?.currency ?? currencyCode, isLoading };
}

/**
 * Every stock transaction, for the buy and sell marks of the portfolio line and its first day,
 * with the converter that values a trade in the report currency at the rates of its own day
 * (`convertAt` is `null` without a rate, and until `ready`).
 */
export function useStockTrades() {
  const { data } = useQuery<InvestmentTransaction[]>({
    queryKey: ['all-stock-transactions'],
    queryFn: () => investmentsApi.getAllTransactions(),
    staleTime: 60 * 1000,
  });
  const transactions = useMemo(() => data ?? [], [data]);
  const { convertAt, ready } = useTransactionFx(data);
  return { transactions, convertAt, ready };
}

/**
 * "Aktualizovat ceny": the desktop's refresh with its toasts (prices, the dividends in the
 * background, a rate limit said as such). Every query built on prices refetches afterwards.
 */
export function useRefreshStockPrices() {
  const { t } = useTranslation('stocks');
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async () => {
      const pricesResult = await priceApi.refreshStockPrices(true);
      priceApi.refreshDividends().catch(console.error);
      return pricesResult;
    },
    onSuccess: (result) => {
      void invalidateAfterPriceRefresh(queryClient);
      queryClient.invalidateQueries({ queryKey: ['dividend-summary'] });
      if (result.rate_limit_hit) {
        toast(t('toast.rateLimitReached'), {
          description: t('toast.rateLimitDescription', {
            updated: result.updated.length,
            remaining: result.remaining_tickers.length,
          }),
        });
      } else {
        toast(t('toast.pricesRefreshed', { count: result.updated.length }), {
          description: t('toast.pricesRefreshedDescription'),
        });
      }
    },
    onError: (error: Error) => {
      toast.error(t('toast.refreshFailed'), { description: error.message });
    },
  });
}

/**
 * The return of the portfolio since 1 January: the time-weighted one (what the positions
 * earned, whatever was bought or sold) and the money-weighted one (what the invested money
 * earned), in percent, from the last day of the performance report. Both are `null` while it
 * loads and while it has fewer than two days: a year that has just begun has no return yet.
 */
export function useTwrThisYear(enabled: boolean): {
  twr: number | null;
  mwr: number | null;
  isLoading: boolean;
} {
  const { currencyCode } = useCurrency();
  const nowSec = Date.now() / 1000;
  const performance = useStockPerformance(
    enabled
      ? {
          ...ytdRange(nowSec),
          reportCurrency: currencyCode,
          series: [],
        }
      : null
  );
  const { data } = performance;
  if (data === undefined || data.dates.length < 2) {
    return { twr: null, mwr: null, isLoading: performance.isLoading };
  }
  return {
    twr: data.portfolio[data.portfolio.length - 1] ?? null,
    mwr: data.mwr[data.mwr.length - 1] ?? null,
    isLoading: performance.isLoading,
  };
}
