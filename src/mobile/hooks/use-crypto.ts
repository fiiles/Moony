import { useMemo } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { CryptoPositions, CryptoTransaction } from '@shared/schema';
import { useTransactionFx } from '@/hooks/use-fx';
import { useCurrency } from '@/lib/currency';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import { cryptoApi, portfolioApi, priceApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type { ChartPeriod } from '@/utils/period';
import { toCryptoRow } from '@/utils/positions';
import { useTickerHistory } from './use-ticker-history';

/** Query keys (prefix-matched) a deleted coin leaves stale, besides the metrics and the cashflow. */
const DELETE_KEYS = [['crypto'], ['all-crypto-transactions'], ['crypto-transactions']] as const;

/**
 * The coins as the backend values them in the report currency (ADR 0011): the query of the
 * desktop's Krypto page and of Majetek (`['crypto', currency, revision]`), so the cache is shared.
 * `rows` are the positions parsed into numbers; `positions` carries the totals and the conversion
 * quality.
 */
export function useCryptoPositions() {
  const { currencyCode, fxRevision } = useCurrency();
  const {
    data: positions,
    isLoading,
    isError,
  } = useQuery<CryptoPositions>({
    queryKey: ['crypto', currencyCode, fxRevision],
    queryFn: () => cryptoApi.getAll(currencyCode),
    refetchOnMount: true,
    staleTime: 60 * 1000,
  });
  const rows = useMemo(() => (positions?.items ?? []).map(toCryptoRow), [positions]);
  return {
    positions,
    rows,
    reportCurrency: positions?.currency ?? currencyCode,
    isLoading,
    /** The query failed (a refetch that fails after data came in keeps the data in `rows`). */
    isError,
  };
}

/**
 * Every crypto transaction, for the buy and sell marks of the class line and its first day, with
 * the converter that values a trade in the report currency at the rates of its own day
 * (`convertAt` is `null` without a rate, and until `ready`).
 */
export function useCryptoTrades() {
  const { data } = useQuery<CryptoTransaction[]>({
    queryKey: ['all-crypto-transactions'],
    queryFn: () => cryptoApi.getAllTransactions(),
    staleTime: 60 * 1000,
  });
  const transactions = useMemo(() => data ?? [], [data]);
  const { convertAt, ready } = useTransactionFx(data);
  return { transactions, convertAt, ready };
}

/**
 * "Aktualizovat ceny": the desktop's refresh with its toasts. Every query built on prices
 * refetches afterwards.
 */
export function useRefreshCryptoPrices() {
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => priceApi.refreshCryptoPrices(true),
    onSuccess: () => {
      void invalidateAfterPriceRefresh(queryClient);
      toast(t('detail.pricesRefreshed'));
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });
}

/**
 * Everything the coin's detail shows (spec §8.8), from the queries of the desktop's
 * `CryptoDetail` and `PositionHero` so the cache is shared: the coin and the class totals come from
 * the list (there is no single-position command), its trades, and the value history of the period
 * (`useTickerHistory`: refetched when the backend finishes a recalculation, and backfilled once for
 * a coin imported without history).
 */
export function useCryptoPosition(id: string | undefined, period: ChartPeriod) {
  const { positions, rows, reportCurrency, isLoading } = useCryptoPositions();
  const holding = useMemo(() => rows.find((row) => row.id === id) ?? null, [rows, id]);

  const { data: transactionData } = useQuery<CryptoTransaction[]>({
    queryKey: ['crypto-transactions', id],
    queryFn: () => cryptoApi.getTransactions(id!),
    enabled: !!id,
  });
  const transactions = useMemo(() => transactionData ?? [], [transactionData]);
  // Each trade converts at the rates of its own day; the hero takes everything from the backend.
  const { ready: ratesReady, convertAt } = useTransactionFx(transactionData);

  const history = useTickerHistory('crypto', holding?.ticker, period);

  return {
    positions,
    /** The coin as the trade and rate dialogs take it (`toCryptoRow`); `null` until loaded or when gone. */
    holding,
    reportCurrency,
    /** Every trade of the coin, in the order the backend gives them. */
    transactions,
    /** Converts a trade at the rates of its own day; `null` without a rate and until `ratesReady`. */
    convertAt,
    ratesReady,
    /** The value and cost line of `period`, each day converted into the report currency. */
    history,
    isLoading,
    /** Loaded, and there is no such coin (a deep link to a deleted one). */
    notFound: !isLoading && holding === null,
  };
}

/**
 * Deletes the coin (the mutation contract — the domain keys, the metrics and the cashflow, then a
 * fresh snapshot and the history). The caller leaves the screen when it has succeeded and then
 * drops the coin's own trades (`CryptoMenu`). The domain keys are refetched only where nothing is
 * showing them: the detail reads the coin from the very list this invalidates, so refetching it
 * under the open screen would swap the coin for "Position not found" until the screen has left;
 * the list that mounts next finds it invalidated and refetches.
 */
export function useDeleteCrypto(id: string | undefined) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');

  return useMutation({
    mutationFn: () => cryptoApi.delete(id!),
    onSuccess: async () => {
      for (const queryKey of DELETE_KEYS) {
        queryClient.invalidateQueries({ queryKey, refetchType: 'inactive' });
      }
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      // The delete already succeeded; a failed snapshot must not turn it into an error.
      try {
        await portfolioApi.recordSnapshot();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (error) {
        console.error('Failed to record portfolio snapshot:', error);
      }
      toast(t('toast.deleted'));
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });
}
