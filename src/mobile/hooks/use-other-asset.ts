import { useCallback, useMemo } from 'react';
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type {
  AssetValuation,
  InsertOtherAsset,
  InsertOtherAssetTransaction,
  OtherAsset,
  OtherAssetTransaction,
} from '@shared/schema';
import { useDatedFx, useDatedMainConvert, useMainConvert } from '@/hooks/use-fx';
import { useCurrency } from '@/lib/currency';
import { otherAssetsApi, portfolioApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { compareKnown } from '@/utils/positions';
import {
  otherAssetRow,
  otherAssetsSummary,
  type OtherAssetBuy,
  type OtherAssetRow,
} from '@/utils/other-assets';

/** The one query of the list: the desktop's Ostatní aktiva page and Majetek read it too. */
const ASSETS_KEY = ['other-assets'] as const;

/** The buys of an asset's transactions, the cost of what is held (`otherAssetRow`); `undefined` until loaded. */
function buysOf(
  transactions: readonly OtherAssetTransaction[] | undefined
): OtherAssetBuy[] | undefined {
  return transactions
    ?.filter((tx) => tx.type === 'buy')
    .map((tx) => ({
      quantity: parseFloat(tx.quantity) || 0,
      pricePerUnit: parseFloat(tx.pricePerUnit) || 0,
      currency: tx.currency,
      date: tx.transactionDate,
    }));
}

/** The first day any rate is needed for: the asset's creation and its earliest trade. */
function firstDayOf(
  assets: readonly OtherAsset[],
  transactions: readonly (readonly OtherAssetTransaction[] | undefined)[]
): number | undefined {
  const days = [
    ...assets.map((asset) => asset.createdAt),
    ...transactions.flatMap((list) => list?.map((tx) => tx.transactionDate) ?? []),
  ];
  return days.length === 0 ? undefined : Math.min(...days);
}

/**
 * Collects the per-asset transaction queries into one result. A stable function, so React Query
 * memoizes the result and its `data` keeps its identity until a query changes.
 */
function combineTransactions(
  results: { data: OtherAssetTransaction[] | undefined; isLoading: boolean }[]
) {
  return {
    data: results.map((result) => result.data),
    loaded: results.every((result) => !result.isLoading),
  };
}

/**
 * The other assets as the desktop's Ostatní aktiva page reads them (`OtherAssets.tsx`, the same
 * queries, so the cache is shared): the list, every asset's transactions (each buy converts at the
 * rates of its own day, which is what the cost is) and the rows `otherAssetRow` derives — native
 * values in each asset's price currency, converted ones in the report currency — with the page
 * totals. `rows` are the highest value first, the ones without a rate last. Until the daily rates
 * and the transactions are in, the cost is pending, not missing (`costReady`).
 */
export function useOtherAssets() {
  const { currencyCode, fxReady } = useCurrency();
  const {
    data,
    isLoading: listLoading,
    isError,
  } = useQuery<OtherAsset[]>({ queryKey: ASSETS_KEY, queryFn: () => otherAssetsApi.getAll() });
  const assets = useMemo(() => data ?? [], [data]);

  const transactions = useQueries({
    queries: assets.map((asset) => ({
      queryKey: ['other-asset-transactions', asset.id],
      queryFn: () => otherAssetsApi.getTransactions(asset.id),
    })),
    combine: combineTransactions,
  });
  const convert = useMainConvert();
  const first = useMemo(() => firstDayOf(assets, transactions.data), [assets, transactions.data]);
  const dated = useDatedMainConvert(first);

  const rows = useMemo(
    () =>
      assets
        .map((asset, i) =>
          otherAssetRow(asset, convert, dated.convertAt, buysOf(transactions.data[i]))
        )
        .sort((a, b) => compareKnown(b.value, a.value)),
    [assets, convert, dated.convertAt, transactions.data]
  );
  const totals = useMemo(() => otherAssetsSummary(rows), [rows]);
  const costReady = dated.ready && transactions.loaded;

  return {
    assets,
    rows,
    totals,
    costReady,
    reportCurrency: currencyCode,
    /** The list, or the rates its values wait for, has not arrived yet. */
    isLoading: listLoading || !fxReady,
    /** The query failed (a refetch that fails after data came in keeps the data in `rows`). */
    isError,
  };
}

/**
 * Everything the asset's detail shows (spec §8.8): the asset from the list (there is no
 * single-asset command), its transactions and its valuations, oldest first — the desktop's keys —
 * and `row`, the asset derived as the list derives it. `convertAt` values a trade at the rates of
 * its own day (`null` without a rate, and until `ratesReady`).
 */
export function useOtherAsset(id: string | undefined) {
  const { currencyCode } = useCurrency();
  const { data: assets, isLoading: listLoading } = useQuery<OtherAsset[]>({
    queryKey: ASSETS_KEY,
    queryFn: () => otherAssetsApi.getAll(),
  });
  const asset = useMemo(() => assets?.find((a) => a.id === id) ?? null, [assets, id]);

  const { data: transactionData, isLoading: transactionsLoading } = useQuery<
    OtherAssetTransaction[]
  >({
    queryKey: ['other-asset-transactions', id],
    queryFn: () => otherAssetsApi.getTransactions(id!),
    enabled: !!id,
  });
  const { data: valuationData, isLoading: valuationsLoading } = useQuery<AssetValuation[]>({
    queryKey: ['other-asset-valuations', id],
    queryFn: () => otherAssetsApi.getValuations(id!),
    enabled: !!id,
  });
  const transactions = useMemo(() => transactionData ?? [], [transactionData]);
  const valuations = useMemo(() => valuationData ?? [], [valuationData]);

  const convert = useMainConvert();
  const first = useMemo(
    () => firstDayOf(asset ? [asset] : [], [transactionData]),
    [asset, transactionData]
  );
  const { ready: ratesReady, convertAt } = useDatedFx(first);
  const convertToReport = useCallback(
    (amount: number, currency: string, timestamp: number) =>
      convertAt(amount, currency, currencyCode, timestamp),
    [convertAt, currencyCode]
  );
  const row = useMemo<OtherAssetRow | null>(
    () => (asset ? otherAssetRow(asset, convert, convertToReport, buysOf(transactionData)) : null),
    [asset, convert, convertToReport, transactionData]
  );

  const isLoading = listLoading || transactionsLoading || valuationsLoading;
  return {
    asset,
    row,
    transactions,
    valuations,
    convertAt,
    ratesReady,
    reportCurrency: currencyCode,
    isLoading,
    /** Loaded, and there is no such asset (a deep link to a deleted one). */
    notFound: !isLoading && asset === null,
  };
}

/**
 * The asset's writes, each following the mutation contract — the domain keys, the metrics and the
 * cashflow, then a fresh snapshot and the history (reference: `use-bank-account-mutations.ts`):
 * - `create` (the add sheet, which has no asset yet: `id` is left out): the asset and, when the
 *   person gave one, its first purchase;
 * - `deleteTransaction` of the open asset;
 * - `remove` the open asset. The page runs it (`useDeleteAndLeave`), which leaves the screen when
 *   it has succeeded and then drops the asset's own queries. The domain keys are refetched only
 *   where nothing is showing them: the detail reads the asset from the very list this invalidates,
 *   so refetching it under the open screen would swap the asset for "not found" until the screen
 *   has left; the list that mounts next finds it invalidated and refetches.
 */
export function useOtherAssetMutations(id?: string) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('otherAssets');
  const { t: ts } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };
  /** What every write leaves stale besides its own keys; the snapshot is best-effort. */
  const refreshTotals = async () => {
    queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
    // The write already succeeded; a failed snapshot must not turn it into an error.
    try {
      await portfolioApi.recordSnapshot();
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
    } catch (error) {
      console.error('Failed to record portfolio snapshot:', error);
    }
  };

  const create = useMutation({
    mutationFn: (variables: {
      data: InsertOtherAsset;
      initialTransaction?: InsertOtherAssetTransaction;
    }) => otherAssetsApi.create(variables.data, variables.initialTransaction),
    onSuccess: async () => {
      queryClient.invalidateQueries({ queryKey: ASSETS_KEY });
      await refreshTotals();
      toast(t('toast.added'));
    },
    onError,
  });

  const deleteTransaction = useMutation({
    mutationFn: (txId: string) => otherAssetsApi.deleteTransaction(txId),
    onSuccess: async () => {
      queryClient.invalidateQueries({ queryKey: ['other-asset-transactions', id] });
      queryClient.invalidateQueries({ queryKey: ASSETS_KEY });
      await refreshTotals();
      toast(tc('status.success'), { description: ts('detail.transactionDeleted') });
    },
    onError,
  });

  const remove = useMutation({
    mutationFn: () => otherAssetsApi.delete(id!),
    onSuccess: async () => {
      for (const queryKey of [
        ASSETS_KEY,
        ['other-asset-transactions', id],
        ['other-asset-valuations', id],
      ]) {
        queryClient.invalidateQueries({ queryKey, refetchType: 'inactive' });
      }
      await refreshTotals();
      toast(t('toast.deleted'));
    },
    onError,
  });

  return { create, deleteTransaction, remove };
}
