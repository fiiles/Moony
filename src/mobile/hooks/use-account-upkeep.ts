import { useQueries, useQuery } from '@tanstack/react-query';
import { useCurrency } from '@/lib/currency';
import { bankAccountsApi, cashflowApi } from '@/lib/tauri-api';
import { utcDayFloor } from '@/utils/chart-axis';
import { newestImportPerAccount } from '@/mobile/lib/last-import';

/**
 * The two queries the phone reads about the state of the accounts, for Údržba (`useUpkeep`) and
 * Finance (`useFinance`) alike. Their keys are the desktop's (Bankovní účty, the account detail),
 * so each is one cache entry shared by all of them and every money write refreshes it
 * (`refreshMoneyFlows`). Change a key here and the desktop must change with it.
 */

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

/**
 * Every booked payment without a category on every account, over all time up to the end of today
 * (UTC), in the main currency: the rows the desktop's Bankovní účty stat counts, so the phone and
 * the desktop agree on the number.
 */
export function useUncategorizedAll() {
  const { currencyCode } = useCurrency();
  const end = utcDayFloor(Date.now() / 1000) + 86400 - 1;
  return useQuery({
    queryKey: ['bank-transactions', 'uncategorized-all', currencyCode, end],
    queryFn: () =>
      cashflowApi.getTransactions(0, end, {
        categoryId: 'uncategorized',
        reportCurrency: currencyCode,
      }),
  });
}
