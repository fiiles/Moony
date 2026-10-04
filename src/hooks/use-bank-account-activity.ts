import { useMemo } from 'react';
import { useQueries } from '@tanstack/react-query';
import type { BankAccountWithInstitution, BankTransaction } from '@shared/schema';
import { bankAccountsApi } from '@/lib/tauri-api';
import { summarizeFlow, type FlowSummary } from '@/utils/bank-activity';

const DAY = 86400;

export interface AccountActivity {
  /** Flow of the last 30 days in the account currency. */
  last30: FlowSummary;
  /** Newest transaction of the account, if any. */
  last: BankTransaction | null;
  isLoading: boolean;
}

/**
 * Recent activity per bank account for the list page: the last 30 days of
 * transactions (change, uncategorized count) and the newest movement. One
 * small query per account for each; accounts are few.
 */
export function useBankAccountActivity(accounts: readonly BankAccountWithInstitution[]) {
  const since = Math.floor(Date.now() / 1000 / DAY) * DAY - 30 * DAY;

  const recent = useQueries({
    queries: accounts.map((account) => ({
      queryKey: ['bank-transactions', account.id, 'recent-30d', since],
      queryFn: () => bankAccountsApi.getTransactions(account.id, { dateFrom: since, limit: 1000 }),
      staleTime: 0,
    })),
  });
  const latest = useQueries({
    queries: accounts.map((account) => ({
      queryKey: ['bank-transactions', account.id, 'latest'],
      queryFn: () => bankAccountsApi.getTransactions(account.id, { limit: 1 }),
      staleTime: 0,
    })),
  });

  const recentSignature = recent.map((q) => q.dataUpdatedAt).join(',');
  const latestSignature = latest.map((q) => q.dataUpdatedAt).join(',');
  const activity = useMemo(() => {
    const map = new Map<string, AccountActivity>();
    accounts.forEach((account, i) => {
      map.set(account.id, {
        last30: summarizeFlow(recent[i]?.data?.transactions ?? []),
        last: latest[i]?.data?.transactions[0] ?? null,
        isLoading: !!recent[i]?.isLoading || !!latest[i]?.isLoading,
      });
    });
    return map;
    // the query arrays are new every render; their data changes exactly with the signatures
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accounts, recentSignature, latestSignature]);

  return activity;
}
