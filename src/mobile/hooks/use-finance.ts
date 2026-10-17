import { useMemo, useState } from 'react';
import { useQueries, useQuery } from '@tanstack/react-query';
import type { BankAccountWithInstitution } from '@shared/schema';
import type { ConvertFn } from '@shared/fx';
import { useBankAccountActivity, type AccountActivity } from '@/hooks/use-bank-account-activity';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useMainConvert } from '@/hooks/use-fx';
import { useInsurance } from '@/hooks/use-insurance';
import { useLoans } from '@/hooks/use-loans';
import { useCurrency } from '@/lib/currency';
import { bankAccountsApi, cashflowApi } from '@/lib/tauri-api';
import { utcDayFloor } from '@/utils/chart-axis';
import { accountTotals } from '@/utils/bank-account-summary';

export interface Finance {
  accounts: BankAccountWithInstitution[];
  /** Each account's 30-day flow and newest movement (native currency). */
  activity: Map<string, AccountActivity>;
  /** Unix seconds of each account's newest import. */
  lastImportByAccount: Record<string, number | null>;
  /**
   * Main-currency totals of the accounts counted in net worth: the balance, the 30-day net, the
   * balances of the excluded accounts, and the currencies left out for want of a rate.
   */
  totals: { balance: number; net30: number; excluded: number; missing: string[] };
  /** Every booked payment without a category on every account, and the accounts that hold them. */
  uncategorized: { count: number; accountIds: string[] };
  loans: ReturnType<typeof useLoans>;
  insurance: ReturnType<typeof useInsurance>;
  mainCurrency: string;
  fxReady: boolean;
  convert: ConvertFn;
  /** The first load is not complete yet (a skeleton, not an empty state); later refetches do not set it. */
  isLoading: boolean;
}

/**
 * What the Finance tab shows (spec §8.4): the accounts with their 30-day activity and totals, the
 * payments without a category, the loans and the policies. Everything comes from the hooks and
 * query keys of the desktop's Bankovní účty, Úvěry and Pojištění pages, so a write on either side
 * (`refreshMoneyFlows`) refreshes both. The uncategorized count is the one `useUpkeep` and
 * `BankAccounts.tsx` read (same key, one cache entry). The first paint waits for all of it — rates
 * included, for until they load a foreign balance would be named as missing — so the page's parts
 * do not trickle in and push it down.
 */
export function useFinance(): Finance {
  const { accounts, isLoading: accountsLoading } = useBankAccounts();
  const { currencyCode, fxReady } = useCurrency();
  const convert = useMainConvert();
  const activity = useBankAccountActivity(accounts);
  const loans = useLoans();
  const insurance = useInsurance();

  const imports = useQueries({
    queries: accounts.map((account) => ({
      queryKey: ['import-batches', account.id],
      queryFn: () => bankAccountsApi.getImportBatches(account.id),
      staleTime: 0,
    })),
  });
  const uncategorizedEnd = utcDayFloor(Date.now() / 1000) + 86400 - 1;
  const uncategorizedQuery = useQuery({
    queryKey: ['bank-transactions', 'uncategorized-all', currencyCode, uncategorizedEnd],
    queryFn: () =>
      cashflowApi.getTransactions(0, uncategorizedEnd, {
        categoryId: 'uncategorized',
        reportCurrency: currencyCode,
      }),
  });

  const lastImportByAccount: Record<string, number | null> = {};
  accounts.forEach((account, i) => {
    const batches = imports[i]?.data ?? [];
    lastImportByAccount[account.id] = batches.length
      ? Math.max(...batches.map((b) => b.importedAt))
      : null;
  });

  const totals = useMemo(() => {
    const sums = accountTotals(
      accounts.map((account) => ({ ...account, last30: activity.get(account.id)?.last30 })),
      convert
    );
    return {
      balance: sums.included,
      net30: sums.income - sums.expense,
      excluded: sums.excluded,
      missing: sums.missing,
    };
  }, [accounts, activity, convert]);

  const uncategorized = useMemo(() => {
    const payments = uncategorizedQuery.data ?? [];
    return {
      count: payments.length,
      accountIds: [...new Set(payments.map((payment) => payment.bankAccountId))],
    };
  }, [uncategorizedQuery.data]);

  const pending =
    accountsLoading ||
    !fxReady ||
    loans.isLoading ||
    insurance.isLoading ||
    uncategorizedQuery.isPending ||
    imports.some((q) => q.isPending) ||
    [...activity.values()].some((a) => a.isLoading);
  // Only the first load is a skeleton: the queries of an account added later must not blank the
  // page (and lose its scroll) while they answer.
  const [loaded, setLoaded] = useState(false);
  if (!pending && !loaded) setLoaded(true);
  const isLoading = pending && !loaded;

  return {
    accounts,
    activity,
    lastImportByAccount,
    totals,
    uncategorized,
    loans,
    insurance,
    mainCurrency: currencyCode,
    fxReady,
    convert,
    isLoading,
  };
}
