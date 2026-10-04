import { useQuery, useQueries } from '@tanstack/react-query';
import { bankAccountsApi } from '@/lib/tauri-api';
import { convertToCzK, type CurrencyCode } from '@shared/currencies';
import type { BankAccountWithInstitution, SavingsAccountZone } from '@shared/schema';
import { calculateZonedInterest } from '@/utils/bank-account-zones';
import { splitBalancesCzk } from '@/utils/bank-account-summary';

export interface BankAccountsMetrics {
  /** Accounts that count toward the portfolio (excluded accounts are left out, like the dashboard) */
  totalBalance: number;
  /** Balance of the accounts the user excluded from the portfolio, in CZK */
  excludedBalance: number;
  excludedCount: number;
  savingsBalance: number;
  checkingBalance: number;
  accountCount: number;
  averageInterestRate: number;
  expectedYearlyInterest: number;
}

export function useBankAccounts() {
  const {
    data: accounts = [],
    isLoading,
    refetch,
  } = useQuery({
    queryKey: ['bank-accounts'],
    queryFn: () => bankAccountsApi.getAll(),
    staleTime: 0, // Always consider stale
    refetchOnWindowFocus: true,
  });

  // Metrics in CZK over the accounts that count toward the portfolio: the
  // dashboard leaves `excludeFromBalance` accounts out, so this page must too
  //. Their balance is reported separately.
  const includedAccounts = accounts.filter((item) => !item.excludeFromBalance);
  const {
    included: totalBalance,
    excluded: excludedBalance,
    excludedCount,
  } = splitBalancesCzk(accounts, (amount, currency) =>
    convertToCzK(amount, currency as CurrencyCode)
  );

  const savingsBalance = includedAccounts
    .filter((item: BankAccountWithInstitution) => item.accountType === 'savings')
    .reduce((sum, item: BankAccountWithInstitution) => {
      const balance = parseFloat(item.balance || '0');
      const currency = item.currency || 'CZK';
      return sum + convertToCzK(balance, currency as CurrencyCode);
    }, 0);

  const checkingBalance = includedAccounts
    .filter((item: BankAccountWithInstitution) => item.accountType === 'checking')
    .reduce((sum, item: BankAccountWithInstitution) => {
      const balance = parseFloat(item.balance || '0');
      const currency = item.currency || 'CZK';
      return sum + convertToCzK(balance, currency as CurrencyCode);
    }, 0);

  // Fetch zones for all zoned accounts
  const zonedAccountIds = includedAccounts
    .filter((item: BankAccountWithInstitution) => item.hasZoneDesignation)
    .map((item: BankAccountWithInstitution) => item.id);

  const zonesQueries = useQueries({
    queries: zonedAccountIds.map((accountId) => ({
      queryKey: ['bank-account-zones', accountId],
      queryFn: () => bankAccountsApi.getZones(accountId),
      staleTime: 0, // Always refetch for fresh data
    })),
  });

  // Create a map of accountId -> zones
  const zonesMap = new Map<string, SavingsAccountZone[]>();
  zonedAccountIds.forEach((accountId, index) => {
    const queryResult = zonesQueries[index];
    if (queryResult.data) {
      zonesMap.set(accountId, queryResult.data);
    }
  });

  // Calculate average interest rate (weighted by balance) and expected yearly interest
  const accountsWithInterest = includedAccounts.filter(
    (item: BankAccountWithInstitution) =>
      (item.interestRate && parseFloat(item.interestRate) > 0) || item.hasZoneDesignation
  );

  let totalWeightedRate = 0;
  let totalBalanceWithInterest = 0;
  let expectedYearlyInterest = 0;

  accountsWithInterest.forEach((item: BankAccountWithInstitution) => {
    const balance = parseFloat(item.balance || '0');
    const currency = item.currency || 'CZK';
    const balanceInCzk = convertToCzK(balance, currency as CurrencyCode);

    if (item.hasZoneDesignation) {
      // For zoned accounts, calculate interest using zones
      const zones = zonesMap.get(item.id);
      if (zones && zones.length > 0) {
        // Calculate yearly interest in original currency using zones
        const yearlyInterestInOriginalCurrency = calculateZonedInterest(balance, zones);
        // Convert to CZK
        const yearlyInterestInCzk = convertToCzK(
          yearlyInterestInOriginalCurrency,
          currency as CurrencyCode
        );
        expectedYearlyInterest += yearlyInterestInCzk;

        // For weighted rate calculation, use effective rate
        const effectiveRate = balance > 0 ? (yearlyInterestInOriginalCurrency / balance) * 100 : 0;
        totalWeightedRate += balanceInCzk * effectiveRate;
        totalBalanceWithInterest += balanceInCzk;
      }
    } else {
      // For non-zoned accounts, use simple calculation
      const rate = parseFloat(item.interestRate || '0');
      totalWeightedRate += balanceInCzk * rate;
      totalBalanceWithInterest += balanceInCzk;
      expectedYearlyInterest += balanceInCzk * (rate / 100);
    }
  });

  const averageInterestRate =
    totalBalanceWithInterest > 0 ? totalWeightedRate / totalBalanceWithInterest : 0;

  const metrics: BankAccountsMetrics = {
    totalBalance,
    excludedBalance,
    excludedCount,
    savingsBalance,
    checkingBalance,
    accountCount: accounts.length,
    averageInterestRate,
    expectedYearlyInterest,
  };

  return {
    accounts,
    metrics,
    isLoading,
    refetch,
  };
}

export function useBankAccount(id: string | undefined) {
  const {
    data: account,
    isLoading,
    refetch,
  } = useQuery({
    queryKey: ['bank-account', id],
    queryFn: () => (id ? bankAccountsApi.get(id) : null),
    enabled: !!id,
  });

  return {
    account,
    isLoading,
    refetch,
  };
}

export function useInstitutions() {
  const { data: institutions = [], isLoading } = useQuery({
    queryKey: ['institutions'],
    queryFn: () => bankAccountsApi.getInstitutions(),
  });

  return {
    institutions,
    isLoading,
  };
}
