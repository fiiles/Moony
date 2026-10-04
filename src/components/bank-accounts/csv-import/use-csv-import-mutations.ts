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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['bank-transactions', accountId] });
      queryClient.invalidateQueries({ queryKey: ['bank-transactions', accountId, 'any'] });
      queryClient.invalidateQueries({ queryKey: ['import-batches', accountId] });
      queryClient.invalidateQueries({ queryKey: ['bank-account', accountId] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      queryClient.invalidateQueries({ queryKey: ['budgeting-report'] });
      // Their duplicate verdicts are out of date now: the rows are in the database.
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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['rulePacks'] });
      queryClient.invalidateQueries({ queryKey: ['packRules'] });
    },
  });
}
