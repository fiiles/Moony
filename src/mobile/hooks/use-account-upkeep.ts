import { useQueries } from '@tanstack/react-query';
import { bankAccountsApi } from '@/lib/tauri-api';
import { newestImportPerAccount } from '@/mobile/lib/last-import';

// The query the phone reads about the state of the accounts, for Údržba (`useUpkeep`) and Finance
// (`useFinance`) alike. Its key is the desktop's (the account detail), so it is one cache entry
// shared by all of them and every money write refreshes it (`refreshMoneyFlows`). Change the key
// here and the desktop must change with it. The payments without a category come from the
// categorize queue (`useCategorizeQueue`).

/**
 * Each account's newest import, in unix seconds (`null`: never imported, or not answered yet).
 * `isPending` holds until every account's history has answered. The histories are always refetched
 * on mount (`staleTime: 0`): a statement imported elsewhere must show up on opening the tab.
 */
export function useLastImports(accounts: ReadonlyArray<{ id: string }>): {
  lastImportByAccount: Record<string, number | null>;
  isPending: boolean;
} {
  const imports = useQueries({
    queries: accounts.map((account) => ({
      queryKey: ['import-batches', account.id],
      queryFn: () => bankAccountsApi.getImportBatches(account.id),
      staleTime: 0,
    })),
  });
  return {
    lastImportByAccount: newestImportPerAccount(
      accounts.map((account) => account.id),
      imports.map((q) => q.data)
    ),
    isPending: imports.some((q) => q.isPending),
  };
}
