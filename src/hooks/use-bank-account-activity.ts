import { getCompleteBankHistory } from '@/lib/bank-history';
import { useMemo } from 'react';
import { useQueries } from '@tanstack/react-query';
import type { BankAccountWithInstitution, BankTransaction } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import { bankAccountsApi } from '@/lib/tauri-api';
import { useDatedFx } from '@/hooks/use-fx';
import { recentWindowStart, summarizeFlow, type FlowSummary } from '@/utils/bank-activity';
import { bookedInAccountCurrency } from '@/utils/money-flows';

export interface AccountActivity {
  /** Flow of the last 30 days in the account currency, other currencies converted at their day. */
  last30: FlowSummary;
  /** Newest transaction of the account, if any. */
  last: BankTransaction | null;
  /** Foreign-currency movements left out of `last30` because their day has no rate. */
  excludedForeignCount: number;
  isLoading: boolean;
}

/**
 * Recent activity per bank account for the list page: the last 30 days of
 * transactions (change, uncategorized count) and the newest movement. One
 * small query per account for each; accounts are few.
 */
export function useBankAccountActivity(accounts: readonly BankAccountWithInstitution[]) {
  const since = recentWindowStart(Date.now() / 1000);

  const recent = useQueries({
    queries: accounts.map((account) => ({
      queryKey: ['bank-transactions', account.id, 'recent-30d', since],
      queryFn: () => getCompleteBankHistory(account.id, { dateFrom: since }),
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

  // Daily rates from the oldest foreign-currency movement on, for every account at once
  const firstForeignDay = useMemo(() => {
    const days = accounts.flatMap((account, i) =>
      (recent[i]?.data?.transactions ?? [])
        .filter((tx) => tx.status === 'booked' && tx.currency !== recordCurrency(account.currency))
        .map((tx) => tx.bookingDate)
    );
    return days.length > 0 ? Math.min(...days) : undefined;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accounts, recentSignature]);
  const { ready: ratesReady, convertAt } = useDatedFx(firstForeignDay);

  const activity = useMemo(() => {
    const map = new Map<string, AccountActivity>();
    accounts.forEach((account, i) => {
      const currency = recordCurrency(account.currency);
      const inAccountCurrency = bookedInAccountCurrency(
        recent[i]?.data?.transactions ?? [],
        currency,
        (amount, from, timestamp) =>
          ratesReady ? convertAt(amount, from, currency, timestamp) : null
      );
      map.set(account.id, {
        last30: summarizeFlow(inAccountCurrency.rows, currency),
        // While the daily rates load nothing is reported as missing
        excludedForeignCount: ratesReady ? inAccountCurrency.missingCount : 0,
        last: latest[i]?.data?.transactions[0] ?? null,
        isLoading: !!recent[i]?.isLoading || !!latest[i]?.isLoading,
      });
    });
    return map;
    // the query arrays are new every render; their data changes exactly with the signatures
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accounts, recentSignature, latestSignature, ratesReady, convertAt]);

  return activity;
}
