import { useEffect, useMemo, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { CategorizeQueue, CategorizeQueueRow, ChangedRow } from '@shared/schema';
import { getCompleteBankHistory } from '@/lib/bank-history';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { categorizationApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { recentCategoryIds } from '@/mobile/lib/category-pick';
import { SNOOZE_DAYS, nextSnapshot, type LocalOutcome } from '@/mobile/lib/categorize-queue';

/**
 * The queue's cache entries: under the `bank-transactions` prefix, so every money write
 * (`refreshMoneyFlows`) refreshes them; a snooze, which writes no ledger fact, refreshes this
 * prefix alone.
 */
export const CATEGORIZE_QUEUE_KEY = ['bank-transactions', 'categorize-queue'] as const;

/**
 * Booked payments without a category, newest first (at most 500, `total` counts all), each with
 * the category the app would suggest; one account's or every account's. A payment put off with
 * "Nechat" is not in it. Údržba, the Finance row and the queue page read the same entry (all
 * accounts), so their counts agree.
 */
export function useCategorizeQueue(accountId?: string) {
  return useQuery({
    queryKey: [...CATEGORIZE_QUEUE_KEY, accountId ?? 'all'],
    queryFn: (): Promise<CategorizeQueue> => categorizationApi.getCategorizeQueue(accountId),
  });
}

/**
 * The rows the queue page lists, kept while the person works (rows never jump): the first answer
 * that is not a stale one being refreshed, and — only once none of them is open — the answer's
 * next rows (`nextSnapshot`). `null` until there is one.
 */
export function useQueueSnapshot(
  live: CategorizeQueue | undefined,
  settled: boolean,
  local: ReadonlyMap<string, LocalOutcome>
): readonly CategorizeQueueRow[] | null {
  const [snapshot, setSnapshot] = useState<readonly CategorizeQueueRow[] | null>(null);
  const next = nextSnapshot(snapshot, live, settled, local);
  if (next !== snapshot) setSnapshot(next);
  return next;
}

/**
 * "Nechat": holds payments back from the queue for 30 days. It writes no ledger fact (no
 * category, no amount), so only the queue's own entries are refreshed — not the whole money-flow
 * set — and the count on Údržba and Finance follows. Its toast offers "Vrátit", which puts the
 * same payments back (`unsnooze_categorization`) and then calls `onUndone` with them, so the page
 * can open their rows again.
 */
export function useSnoozeCategorization(onUndone?: (transactionIds: string[]) => void) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('mobile');
  const { t: tm } = useTranslation('milestones');
  const { t: tc } = useTranslation('common');
  const unsnooze = useUnsnoozeCategorization({ quiet: true });
  // The toast outlives a render: it reads the newest callback
  const undone = useRef(onUndone);
  useEffect(() => {
    undone.current = onUndone;
  });
  return useMutation({
    mutationFn: (transactionIds: string[]) =>
      categorizationApi.snoozeCategorization(transactionIds, SNOOZE_DAYS),
    onSuccess: async (_held, transactionIds) => {
      await queryClient.invalidateQueries({ queryKey: CATEGORIZE_QUEUE_KEY });
      toast(t('queue.snoozed', { days: SNOOZE_DAYS }), {
        action: {
          label: tm('actions.undo'),
          onClick: () =>
            unsnooze.mutate(
              { transactionIds },
              { onSuccess: () => undone.current?.(transactionIds) }
            ),
        },
      });
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });
}

/**
 * Puts held-back payments back into the queue: the named ones, or every running snooze of one
 * account (or of all, without `accountId`) — "Vrátit do fronty". The queue's entries refresh; a
 * toast says how many came back unless `quiet` (the "Vrátit" of a single "Nechat").
 */
export function useUnsnoozeCategorization({ quiet = false }: { quiet?: boolean } = {}) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  return useMutation({
    mutationFn: ({
      transactionIds,
      accountId,
    }: {
      transactionIds?: string[];
      accountId?: string;
    }) => categorizationApi.unsnoozeCategorization(transactionIds, accountId),
    onSuccess: async (count) => {
      await queryClient.invalidateQueries({ queryKey: CATEGORIZE_QUEUE_KEY });
      if (!quiet) toast(t('queue.unsnoozed', { count }));
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });
}

/**
 * The expansion's switch turned off: the payments a remembered pick took along get back what they
 * held (`restoreTransactionCategories`), the money flows refresh, a toast says how many. Resolves
 * `true` once restored; a failure is a toast and `false`.
 */
export function useRestoreCategories() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const failed = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  return async (changed: ChangedRow[]): Promise<boolean> => {
    try {
      const restored = await categorizationApi.restoreTransactionCategories(changed);
      toast(t('categorization.historyRestored', { count: restored }));
    } catch (error) {
      failed(error);
      return false;
    }
    // The rows are restored; a failed refresh must not read as a failed restore
    try {
      await refreshMoneyFlows(queryClient);
    } catch (error) {
      failed(error);
    }
    return true;
  };
}

/**
 * The categories of an account's newest categorized rows, newest first, at most five (the
 * picker's "Naposledy použité"). The account's whole history, as the account detail reads it (the
 * same cache entry); loaded only while `accountId` is set, i.e. while the picker is open.
 */
export function useRecentCategoryIds(accountId: string | undefined): string[] {
  const { data } = useQuery({
    queryKey: ['bank-transactions', accountId, 'history'],
    queryFn: () => (accountId ? getCompleteBankHistory(accountId) : null),
    enabled: !!accountId,
  });
  return useMemo(() => recentCategoryIds(data?.transactions ?? [], 5), [data]);
}
