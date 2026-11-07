import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { bankAccountsApi } from '@/lib/tauri-api';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { translateApiError } from '@/lib/translate-api-error';

/**
 * Takes an import back (the desktop's `deleteBatchMutation`): deletes the batch, refreshes every
 * money-flow query and says how many records went. The mutation's variable is the batch id; its
 * result is the number of records removed. The caller closes its confirm sheet on success.
 */
export function useUndoImport() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');

  return useMutation<number, Error, string>({
    mutationFn: (batchId) => bankAccountsApi.deleteImportBatch(batchId),
    onSuccess: async (count) => {
      await refreshMoneyFlows(queryClient);
      toast(t('readiness.batchDeleted', { count }));
    },
    onError: (error) => {
      toast.error(t('messages.error'), { description: translateApiError(error, tc) });
    },
  });
}
