import { useCallback, useMemo } from 'react';
import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import type { BankTransaction, EffectiveTransactionFlow } from '@shared/schema';
import { bankAccountsApi, cashflowApi } from '@/lib/tauri-api';
import { utcDayFloor } from '@/utils/chart-axis';
import { ledgerFilters, nextOffset, type LedgerState } from '@/mobile/lib/ledger';

/** Rows per ledger page (the desktop's page size). */
const PAGE_SIZE = 100;

export interface Ledger {
  /** The loaded rows of every page, newest first. */
  rows: BankTransaction[];
  /** Rows matching the filter, loaded or not. */
  total: number;
  hasMore: boolean;
  /** Loads the next page; a no-op while one is loading or when there is none. */
  loadMore: () => void;
  /** The first page of this filter is loading. */
  isLoading: boolean;
  isFetchingMore: boolean;
  isError: boolean;
  retry: () => void;
  /** The effective meaning of each loaded row (refund, transfer…), by transaction id. */
  flowById: Map<string, EffectiveTransactionFlow>;
}

/**
 * The account detail's ledger with infinite scroll (spec §8.4: it replaces the desktop's paging):
 * pages of 100 rows of the filtered transactions, newest first, under the `bank-transactions` key
 * that every write refreshes. The search arrives debounced from the page. The filter's "now" is
 * today's UTC day, so the key holds still within a day.
 */
export function useLedger(accountId: string | undefined, state: LedgerState): Ledger {
  const today = utcDayFloor(Date.now() / 1000);
  const filters = useMemo(() => ledgerFilters(state, today), [state, today]);

  const query = useInfiniteQuery({
    queryKey: ['bank-transactions', accountId, 'ledger', filters],
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      bankAccountsApi.getTransactions(accountId as string, {
        ...filters,
        limit: PAGE_SIZE,
        offset: pageParam,
      }),
    // An empty page before the total is reached would ask for the same offset forever
    getNextPageParam: (last, pages) =>
      last.transactions.length === 0
        ? undefined
        : nextOffset(
            pages.reduce((n, page) => n + page.transactions.length, 0),
            last.total
          ),
    enabled: !!accountId,
  });
  const { data, hasNextPage, isFetchingNextPage, fetchNextPage, refetch } = query;

  const rows = useMemo(() => data?.pages.flatMap((page) => page.transactions) ?? [], [data]);
  const total = data?.pages[data.pages.length - 1]?.total ?? 0;

  const ids = useMemo(() => rows.map((tx) => tx.id), [rows]);
  const { data: flows } = useQuery({
    queryKey: ['effective-transaction-flows', ids],
    queryFn: () => cashflowApi.getEffectiveFlows(ids),
    enabled: ids.length > 0,
    // The rows already shown keep their badges while a new page's meanings load
    placeholderData: (previous) => previous,
  });
  const flowById = useMemo(() => new Map((flows ?? []).map((flow) => [flow.id, flow])), [flows]);

  // Stable, so the page's scroll observer is not rebuilt on every render
  const loadMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  const retry = useCallback(() => void refetch(), [refetch]);

  return {
    rows,
    total,
    hasMore: hasNextPage,
    loadMore,
    isLoading: query.isLoading,
    isFetchingMore: isFetchingNextPage,
    isError: query.isError,
    retry,
    flowById,
  };
}
