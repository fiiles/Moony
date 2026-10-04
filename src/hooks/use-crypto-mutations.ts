import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { cryptoApi, portfolioApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

/**
 * Query keys (prefix-matched) whose data is derived from crypto transactions.
 * Mirrors the `crypto` domain list in SyncProvider.
 */
const CRYPTO_TRANSACTION_KEYS = [
  ['crypto'],
  ['crypto-detail'],
  ['crypto-transactions'],
  ['all-crypto-transactions'],
] as const;

/**
 * Delete a single crypto transaction. Follows the mutation contract: domain keys
 * + portfolio-metrics + cashflow-report, then a fresh snapshot and
 * portfolio-history (reference: use-bank-account-mutations.ts).
 */
export function useCryptoTransactionMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('crypto');
  const { t: tc } = useTranslation('common');

  const deleteTransaction = useMutation({
    mutationFn: (txId: string) => cryptoApi.deleteTransaction(txId),
    onSuccess: async () => {
      for (const queryKey of CRYPTO_TRANSACTION_KEYS) {
        queryClient.invalidateQueries({ queryKey });
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
      toast(tc('status.success'), { description: t('detail.transactionDeleted') });
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  return { deleteTransaction };
}
