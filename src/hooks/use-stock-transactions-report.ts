import { keepPreviousData, useQuery } from '@tanstack/react-query';
import type { StockTransactionsReport, StockTransactionsReportRequest } from '@shared/schema';
import { stockTransactionsApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';

/**
 * The stock transactions report for one page state (period, report currency, cost method,
 * holding test). The rate revision is part of the key; stock writes and price refreshes
 * invalidate the `['stock-transactions-report']` prefix. Pass `null` while there is nothing to
 * request. The previous response stays in `data` while the next one loads (`isPlaceholderData`).
 */
export function useStockTransactionsReport(request: StockTransactionsReportRequest | null) {
  const { fxRevision } = useCurrency();
  const query = useQuery<StockTransactionsReport>({
    queryKey: ['stock-transactions-report', request, fxRevision],
    queryFn: () => {
      if (!request) throw new Error('report request missing');
      return stockTransactionsApi.getReport(request);
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
