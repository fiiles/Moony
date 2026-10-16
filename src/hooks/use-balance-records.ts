import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
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

/** After any balance write: the money-flow refresh contract (rule 7) and the milestones (stale balances). */
async function refreshAfterBalanceWrite(queryClient: QueryClient) {
  await refreshMoneyFlows(queryClient);
  await queryClient.invalidateQueries({ queryKey: ['milestones'] });
}

/**
 * Record or delete a dated balance. Both refresh the money-flow queries, record a
 * portfolio snapshot (rule 7) and the milestones (stale balances).
 */
export function useBalanceRecordMutations(accountId?: string) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');

  const afterWrite = () => refreshAfterBalanceWrite(queryClient);

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

export interface AccountBalanceRecord {
  accountId: string;
  data: InsertBalanceRecord;
}

/**
 * Record a dated balance on several accounts at once (the phone's "Zaznamenat zůstatky" sheet):
 * the writes are `recordBalance`'s, one after the other, followed by one refresh and no success
 * toast of its own (the caller says one thing for all of them). The refresh also runs after a
 * failure, since the writes before it went through; the same day replaces the same day, so
 * saving again is safe.
 */
export function useRecordBalances() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');

  return useMutation({
    mutationFn: async (records: readonly AccountBalanceRecord[]) => {
      for (const { accountId, data } of records) {
        await bankAccountsApi.recordBalance(accountId, data);
      }
    },
    onSettled: () => refreshAfterBalanceWrite(queryClient),
    onError: (error: Error) => {
      toast.error(t('messages.error'), { description: translateApiError(error, tc) });
    },
  });
}
