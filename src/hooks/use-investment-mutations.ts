import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { investmentsApi, portfolioApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type { InsertInvestmentTransaction } from '@shared/schema';

/**
 * Query keys (prefix-matched) whose data is derived from stock transactions.
 * Mirrors the `stocks` domain list in SyncProvider plus the analysis views.
 */
const STOCK_TRANSACTION_KEYS = [
  ['investments'],
  ['investment'],
  ['investment-transactions'],
  ['transactions'],
  ['all-stock-transactions'],
  ['dividend-summary'],
  ['stocks-analysis'],
  ['tag-metrics'],
  ['stock-twr'],
] as const;

/**
 * Edit / delete a single stock transaction. Follows the mutation contract:
 * domain keys + portfolio-metrics + cashflow-report, then a fresh snapshot and
 * portfolio-history (reference: use-bank-account-mutations.ts).
 */
export function useInvestmentTransactionMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('stocks');
  const { t: tc } = useTranslation('common');

  const refreshAfterChange = async () => {
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
