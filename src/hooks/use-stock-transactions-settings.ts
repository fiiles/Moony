import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import type { StockTransactionsReportSettings } from '@shared/schema';
import { stockTransactionsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

const KEY = ['stock-transactions-settings'];

const DEFAULT_SETTINGS: StockTransactionsReportSettings = {
  costMethod: 'fifo',
  holdingTestEnabled: false,
  holdingTestMonths: 36,
};

/**
 * The remembered cost method and holding test of the report. `save` updates the cache at once
 * and rolls back with an error toast when the write fails. A preference write: it invalidates
 * only its own key (the report refetches because its request changes) and records no snapshot.
 */
export function useStockTransactionsSettings() {
  const queryClient = useQueryClient();
  const { t: tc } = useTranslation('common');
  const query = useQuery<StockTransactionsReportSettings>({
    queryKey: KEY,
    queryFn: () => stockTransactionsApi.getSettings(),
  });

  const mutation = useMutation({
    mutationFn: (next: StockTransactionsReportSettings) => stockTransactionsApi.saveSettings(next),
    onMutate: async (next) => {
      await queryClient.cancelQueries({ queryKey: KEY });
      const previous = queryClient.getQueryData<StockTransactionsReportSettings>(KEY);
      queryClient.setQueryData(KEY, next);
      return { previous };
    },
    onError: (error: Error, _next, context) => {
      queryClient.setQueryData(KEY, context?.previous);
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: KEY }),
  });

  return {
    settings: query.data ?? DEFAULT_SETTINGS,
    isLoading: query.isLoading,
    save: (next: StockTransactionsReportSettings) => mutation.mutate(next),
  };
}
