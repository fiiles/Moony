import { useMemo, useState } from 'react';
import type { BankAccountWithInstitution } from '@shared/schema';
import type { ConvertFn } from '@shared/fx';
import { useBankAccountActivity, type AccountActivity } from '@/hooks/use-bank-account-activity';
import { useBankAccounts } from '@/hooks/use-bank-accounts';
import { useMainConvert } from '@/hooks/use-fx';
import { useInsurance } from '@/hooks/use-insurance';
import { useLoans } from '@/hooks/use-loans';
import { useCurrency } from '@/lib/currency';
import { accountTotals } from '@/utils/bank-account-summary';
import { useLastImports } from '@/mobile/hooks/use-account-upkeep';
import { useCategorizeQueue } from '@/mobile/hooks/use-categorize-queue';
import { queueSummary } from '@/mobile/lib/categorize-queue';

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
  /** Every booked payment without a category that is not put off (the queue's total), and the accounts that hold them. */
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
 * (`refreshMoneyFlows`) refreshes both. The uncategorized count is the categorize queue's, the one
 * `useUpkeep` reads (same key, one cache entry). The first paint waits for all of it — rates
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

  const { lastImportByAccount, isPending: importsPending } = useLastImports(accounts);
  const queueQuery = useCategorizeQueue();

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

  const uncategorized = useMemo(() => queueSummary(queueQuery.data), [queueQuery.data]);

  const pending =
    accountsLoading ||
    !fxReady ||
    loans.isLoading ||
    insurance.isLoading ||
    queueQuery.isPending ||
    importsPending ||
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
