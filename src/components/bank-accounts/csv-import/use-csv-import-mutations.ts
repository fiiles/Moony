import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { bankAccountsApi, categorizationApi } from '@/lib/tauri-api';
import type { CsvImportConfigInput } from '@shared/schema';

/**
 * Writes a CSV statement. The import changes transactions, never the account
 * balance (that stays a manual value), so besides the transaction
 * lists it refreshes what is derived from them: portfolio metrics and the
 * cashflow / budgeting reports.
 */
export function useCsvImportMutation(accountId: string) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ filePath, config }: { filePath: string; config: CsvImportConfigInput }) =>
      bankAccountsApi.importCsvTransactions(accountId, filePath, config),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      queryClient.removeQueries({ queryKey: ['csv-import-preview'] });
    },
  });
}

/**
 * Turns a country's locale rule pack on. The enabled set is read right
 * before it is written: Settings may have changed it since the dialog opened,
 * and `setRulePacksEnabled` replaces the whole set.
 */
export function useEnableRulePack() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (packId: string) => {
      const packs = await categorizationApi.getRulePacks();
      const enabled = packs.filter((pack) => pack.enabled).map((pack) => pack.packId);
      await categorizationApi.setRulePacksEnabled([...new Set([...enabled, packId])]);
    },
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
    },
  });
}

export function useRecordBalance(accountId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (data: import('@shared/schema').InsertBalanceRecord) =>
      bankAccountsApi.recordBalance(accountId, data),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
    },
  });
}
