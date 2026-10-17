import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useMilestones } from '@/hooks/use-milestones';
import { utcDayFloor } from '@/utils/chart-axis';
import { useLastImports, useUncategorizedAll } from '@/mobile/hooks/use-account-upkeep';
import { buildUpkeepRows, type UpkeepRow } from '@/mobile/lib/upkeep';

export interface Upkeep {
  /** Everything that is due, in card order; empty while `ready` is false. */
  rows: UpkeepRow[];
  /** False until the milestones, accounts, import histories and uncategorized payments have all answered. */
  ready: boolean;
  /** The accounts by id (the card names them), with their institution. */
  accounts: ReturnType<typeof useBankAccounts>['accounts'];
  /** Unix seconds of each account's newest import. */
  lastImportByAccount: Record<string, number | null>;
}

/**
 * What the Údržba card on Přehled shows (spec §8.3 item 1): the milestones plus what the phone
 * works out itself — the last import of each account and the payments without a category. Those
 * are counted as the desktop's Bankovní účty stat counts them (`BankAccounts.tsx`: every booked
 * uncategorized payment on every account, same query key, so the two share one cache entry and
 * agree); Phase 2's categorize-queue command replaces this count later. The keys are those of the
 * desktop, so every money write refreshes them (`refreshMoneyFlows`). The card waits for all of it,
 * so its rows do not trickle in and push the page down.
 */
export function useUpkeep(): Upkeep {
  const { data: milestones = [], isPending: milestonesPending } = useMilestones();
  const { accounts, isLoading: accountsLoading } = useBankAccounts();
  const { lastImportByAccount, isPending: importsPending } = useLastImports(accounts);
  const uncategorized = useUncategorizedAll();

  const ready =
    !milestonesPending && !accountsLoading && !uncategorized.isPending && !importsPending;

  const payments = uncategorized.data ?? [];
  const withPayments = [...new Set(payments.map((payment) => payment.bankAccountId))];

  const rows = ready
    ? buildUpkeepRows({
        todayDay: utcDayFloor(Date.now() / 1000),
        milestones,
        accounts,
        lastImportByAccount,
        uncategorized: payments.length,
        uncategorizedAccountIds: withPayments,
      })
    : [];

  return { rows, ready, accounts, lastImportByAccount };
}
