import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { BankBalanceRecord, InsertBalanceRecord } from '@shared/schema';
import { bankAccountsApi } from '@/lib/tauri-api';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { translateApiError } from '@/lib/translate-api-error';

/** Dated balances of one account, newest first. */
export function useBalanceRecords(accountId?: string) {
  const { data: records = [], isLoading } = useQuery({
    queryKey: ['bank-balance-records', accountId],
    queryFn: () => (accountId ? bankAccountsApi.getBalanceRecords(accountId) : []),
    enabled: !!accountId,
  });
  return { records: records as BankBalanceRecord[], isLoading };
}

/**
 * Record or delete a dated balance. Both refresh the money-flow queries, record a
 * portfolio snapshot (rule 7) and the milestones (stale balances).
 */
export function useBalanceRecordMutations(accountId?: string) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');

  const afterWrite = async () => {
    await refreshMoneyFlows(queryClient);
    await queryClient.invalidateQueries({ queryKey: ['milestones'] });
  };

  const recordBalance = useMutation({
    mutationFn: (data: InsertBalanceRecord) => {
      if (!accountId) throw new Error('validation.accountRequired');
      return bankAccountsApi.recordBalance(accountId, data);
    },
    onSuccess: async () => {
      await afterWrite();
      toast(t('recordDialog.success'));
    },
    onError: (error: Error) => {
      toast.error(t('messages.error'), { description: translateApiError(error, tc) });
    },
  });

  const deleteRecord = useMutation({
    mutationFn: (recordId: string) => bankAccountsApi.deleteBalanceRecord(recordId),
    onSuccess: async () => {
      await afterWrite();
      toast(t('balanceHistory.deleted'));
    },
    onError: (error: Error) => {
      toast.error(t('messages.error'), { description: translateApiError(error, tc) });
    },
  });

  return { recordBalance, deleteRecord };
}
