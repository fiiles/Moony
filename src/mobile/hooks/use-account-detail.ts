import { useCallback, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import type {
  BankAccountWithInstitution,
  BankBalanceRecord,
  BankTransaction,
  CsvImportBatch,
  SavingsAccountZone,
} from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import { useBalanceRecords } from '@/hooks/use-balance-records';
import { useBankAccount } from '@/hooks/use-bank-accounts';
import { useDatedFx } from '@/hooks/use-fx';
import { getCompleteBankHistory } from '@/lib/bank-history';
import { bankAccountsApi } from '@/lib/tauri-api';
import { bookedInAccountCurrency, type AccountConvertAt } from '@/utils/money-flows';

export interface AccountDetailData {
  /** `undefined` while loading, `null` when the account does not exist. */
  account: BankAccountWithInstitution | null | undefined;
  /** Dated balances, newest first. */
  records: BankBalanceRecord[];
  zones: SavingsAccountZone[];
  /** Every transaction of the account, any currency and status (empty until loaded). */
  history: BankTransaction[];
  /** Booked rows in the account currency (a foreign pocket converted at its day's rate). */
  historyRows: BankTransaction[];
  /** Currencies whose rows were left out of `historyRows` for want of a rate (once the rates loaded). */
  foreignMissing: string[];
  batches: CsvImportBatch[];
  /** Unix seconds of the newest import, `null` without one. */
  lastImport: number | null;
  /** The account currency (`recordCurrency` of the stored code). */
  currency: string;
  /** The account itself is loading (the page shows its skeleton). */
  isLoading: boolean;
  /** The complete history has loaded (the flows strip and the "no data" state wait for it). */
  historyLoaded: boolean;
}

/**
 * What the phone's account detail shows besides its ledger (spec §8.4): the account, its balance
 * records and interest zones, the complete history for the flows strip, and the import batches.
 * The keys are the desktop detail's, so a write on either side (`refreshMoneyFlows`) refreshes
 * both; the foreign-pocket conversion is `BankAccountDetail.tsx`'s.
 */
export function useAccountDetail(accountId: string | undefined): AccountDetailData {
  const { account, isLoading } = useBankAccount(accountId);
  const { records } = useBalanceRecords(accountId);

  const { data: zones } = useQuery({
    queryKey: ['bank-account-zones', accountId],
    queryFn: () => (accountId ? bankAccountsApi.getZones(accountId) : null),
    enabled: !!accountId && !!account?.hasZoneDesignation,
  });

  const { data: historyResult, isSuccess: historyLoaded } = useQuery({
    queryKey: ['bank-transactions', accountId, 'history'],
    queryFn: () => (accountId ? getCompleteBankHistory(accountId) : null),
    enabled: !!accountId,
  });

  const { data: batches } = useQuery({
    queryKey: ['import-batches', accountId],
    queryFn: () => (accountId ? bankAccountsApi.getImportBatches(accountId) : null),
    enabled: !!accountId,
  });

  const currency = recordCurrency(account?.currency);
  const history = useMemo(() => historyResult?.transactions ?? [], [historyResult]);

  // Rows of another currency pocket count in the account currency at their day's rate
  const firstForeignDay = useMemo(() => {
    const days = history
      .filter((tx) => tx.status === 'booked' && tx.currency !== currency)
      .map((tx) => tx.bookingDate);
    return days.length > 0 ? Math.min(...days) : undefined;
  }, [history, currency]);
  const { ready: foreignRatesReady, convertAt } = useDatedFx(firstForeignDay);
  const toAccountCurrency = useCallback<AccountConvertAt>(
    (amount, from, timestamp) =>
      foreignRatesReady ? convertAt(amount, from, currency, timestamp) : null,
    [foreignRatesReady, convertAt, currency]
  );
  const converted = useMemo(
    () => bookedInAccountCurrency(history, currency, toAccountCurrency),
    [history, currency, toAccountCurrency]
  );

  const lastImport = useMemo(
    () =>
      (batches ?? []).reduce<number | null>(
        (max, b) => (max === null || b.importedAt > max ? b.importedAt : max),
        null
      ),
    [batches]
  );

  return {
    account,
    records,
    zones: zones ?? [],
    history,
    historyRows: converted.rows,
    // Only once the daily rates have loaded, so loading never reads as missing
    foreignMissing: foreignRatesReady ? converted.missing : [],
    batches: batches ?? [],
    lastImport,
    currency,
    isLoading,
    historyLoaded,
  };
}
