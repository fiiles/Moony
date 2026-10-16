import { useQueries } from '@tanstack/react-query';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useMilestones } from '@/hooks/use-milestones';
import { bankAccountsApi } from '@/lib/tauri-api';
import { utcDayFloor } from '@/utils/chart-axis';
import { buildUpkeepRows, type UpkeepRow } from '@/mobile/lib/upkeep';

export interface Upkeep {
  /** Everything that is due, in card order; empty while `ready` is false. */
  rows: UpkeepRow[];
  /** False until the milestones, accounts, import histories and payment counts have all answered. */
  ready: boolean;
  /** The accounts by id (the card names them), with their institution. */
  accounts: ReturnType<typeof useBankAccounts>['accounts'];
  /** Unix seconds of each account's newest import. */
  lastImportByAccount: Record<string, number | null>;
}

/**
 * What the Údržba card on Přehled shows (spec §8.3 item 1): the milestones plus what the phone
 * works out itself — the last import of each account and the payments without a category (a count
 * from the ledger's own `uncategorized` filter, one row of data per account). The keys are those of
 * the desktop ledger, so every money write refreshes them (`refreshMoneyFlows`). The card waits
 * for all of it, so its rows do not trickle in and push the page down.
 */
export function useUpkeep(): Upkeep {
  const { data: milestones = [], isPending: milestonesPending } = useMilestones();
  const { accounts, isLoading: accountsLoading } = useBankAccounts();

  const imports = useQueries({
    queries: accounts.map((account) => ({
      queryKey: ['import-batches', account.id],
      queryFn: () => bankAccountsApi.getImportBatches(account.id),
      staleTime: 0,
    })),
  });
  const uncategorized = useQueries({
    queries: accounts.map((account) => ({
      queryKey: ['bank-transactions', account.id, 'uncategorized-count'],
      queryFn: async () =>
        (
          await bankAccountsApi.getTransactions(account.id, {
            categoryId: 'uncategorized',
            limit: 1,
          })
        ).total,
      staleTime: 0,
    })),
  });

  const ready =
    !milestonesPending &&
    !accountsLoading &&
    imports.every((q) => !q.isPending) &&
    uncategorized.every((q) => !q.isPending);

  const lastImportByAccount: Record<string, number | null> = {};
  const withPayments: string[] = [];
  let total = 0;
  accounts.forEach((account, i) => {
    const batches = imports[i]?.data ?? [];
    lastImportByAccount[account.id] = batches.length
      ? Math.max(...batches.map((b) => b.importedAt))
      : null;
    const count = uncategorized[i]?.data ?? 0;
    if (count > 0) withPayments.push(account.id);
    total += count;
  });

  const rows = ready
    ? buildUpkeepRows({
        todayDay: utcDayFloor(Date.now() / 1000),
        milestones,
        accounts,
        lastImportByAccount,
        uncategorized: total,
        uncategorizedAccountIds: withPayments,
      })
    : [];

  return { rows, ready, accounts, lastImportByAccount };
}
