import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { investmentsApi, portfolioApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type { InsertInvestmentTransaction } from '@shared/schema';

/**
 * Query keys (prefix-matched) whose data is derived from stock transactions.
 * Mirrors the `stocks` domain list in SyncProvider plus the analysis views and the
 * split rows (a split is listed only while transactions are dated before it).
 */
export const STOCK_TRANSACTION_KEYS = [
  ['investments'],
  ['investment'],
  ['investment-transactions'],
  ['stock-dividends'],
  ['transactions'],
  ['all-stock-transactions'],
  ['dividend-summary'],
  ['stocks-analysis'],
  ['stock-performance'],
  ['stock-splits'],
] as const;

/**
 * The refresh every stock-transaction write runs on success: domain keys +
 * portfolio-metrics + cashflow-report, then a fresh snapshot and portfolio-history
 * (reference: use-bank-account-mutations.ts).
 */
export function useRefreshAfterStockTransaction() {
  const queryClient = useQueryClient();

  return async () => {
    for (const queryKey of STOCK_TRANSACTION_KEYS) {
      queryClient.invalidateQueries({ queryKey });
    }
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
}

/** Edit / delete a single stock transaction. */
export function useInvestmentTransactionMutations() {
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');
  const refreshAfterChange = useRefreshAfterStockTransaction();

  const updateTransaction = useMutation({
    mutationFn: ({ txId, data }: { txId: string; data: Partial<InsertInvestmentTransaction> }) =>
      investmentsApi.updateTransaction(txId, data),
    onSuccess: async () => {
      await refreshAfterChange();
      toast(tc('status.success'), { description: t('detail.transactionUpdated') });
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  const deleteTransaction = useMutation({
    mutationFn: (txId: string) => investmentsApi.deleteTransaction(txId),
    onSuccess: async () => {
      await refreshAfterChange();
      toast(tc('status.success'), { description: t('detail.transactionDeleted') });
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  return { updateTransaction, deleteTransaction };
}
