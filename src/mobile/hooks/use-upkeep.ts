import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useMilestones } from '@/hooks/use-milestones';
import { utcDayFloor } from '@/utils/chart-axis';
import { useLastImports } from '@/mobile/hooks/use-account-upkeep';
import { useCategorizeQueue } from '@/mobile/hooks/use-categorize-queue';
import { queueSummary } from '@/mobile/lib/categorize-queue';
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
 * are counted from the categorize queue (`useCategorizeQueue`: every booked uncategorized payment
 * on every account that is not put off with "Nechat" — the number the queue page opens with — and
 * the accounts that hold the newest 500 of them). The keys sit under the desktop's, so every money
 * write refreshes them (`refreshMoneyFlows`). The card waits for all of it, so its rows do not
 * trickle in and push the page down.
 */
export function useUpkeep(): Upkeep {
  const { data: milestones = [], isPending: milestonesPending } = useMilestones();
  const { accounts, isLoading: accountsLoading } = useBankAccounts();
  const { lastImportByAccount, isPending: importsPending } = useLastImports(accounts);
  const queue = useCategorizeQueue();

  const ready = !milestonesPending && !accountsLoading && !queue.isPending && !importsPending;

  const { count: uncategorized, accountIds: withPayments } = queueSummary(queue.data);

  const rows = ready
    ? buildUpkeepRows({
        todayDay: utcDayFloor(Date.now() / 1000),
        milestones,
        accounts,
        lastImportByAccount,
        uncategorized,
        uncategorizedAccountIds: withPayments,
      })
    : [];

  return { rows, ready, accounts, lastImportByAccount };
}
