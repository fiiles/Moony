import { useCallback } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { CategorizationResult } from '@shared/schema';
import { getCategoryId, toTransactionInput } from '@/hooks/useCategorization';
import { getCompleteBankHistory } from '@/lib/bank-history';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { bankAccountsApi, categorizationApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

/**
 * Runs the rules over the account's payments without a category and saves the matches, after the
 * import's "Zapnout balíček pravidel". Mirrors the desktop account page's "Auto-categorize"
 * (`BankAccountDetail.handleAutoCategorize`, a closure that cannot be imported) — change them
 * together; the payment mapping and the match lookup are the desktop's own helpers
 * (`useCategorization`). Resolves with how many payments got a category; a failure is said in a
 * toast and resolves 0. Matches are written in chunks of 50, so a whole history stays bounded.
 */
export function useAutoCategorize(): (accountId: string) => Promise<number> {
  const queryClient = useQueryClient();
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');

  return useCallback(
    async (accountId: string) => {
      let uncategorizedIds: string[];
      let results: CategorizationResult[];
      try {
        const { transactions } = await getCompleteBankHistory(accountId, {
          categoryId: 'uncategorized',
        });
        if (transactions.length === 0) return 0;
        uncategorizedIds = transactions.map((tx) => tx.id);
        results = await categorizationApi.categorizeBatch(transactions.map(toTransactionInput));
      } catch (error) {
        toast.error(translateApiError(error as Error, tc));
        return 0;
      }

      const matches: Array<{ txId: string; categoryId: string }> = [];
      uncategorizedIds.forEach((txId, i) => {
        const categoryId = results[i] ? getCategoryId(results[i]) : null;
        if (categoryId) matches.push({ txId, categoryId });
      });

      const writes: PromiseSettledResult<void>[] = [];
      for (let i = 0; i < matches.length; i += 50) {
        writes.push(
          ...(await Promise.allSettled(
            matches
              .slice(i, i + 50)
              .map(({ txId, categoryId }) =>
                bankAccountsApi.updateTransactionCategory(txId, categoryId)
              )
          ))
        );
      }
      const succeeded = writes.filter((write) => write.status === 'fulfilled').length;
      const failed = writes.length - succeeded;
      try {
        if (succeeded) await refreshMoneyFlows(queryClient);
      } catch (error) {
        toast.error(translateApiError(error as Error, tc));
      }
      toast(t('readiness.autoResult', { succeeded, failed }));
      if (matches.length === 0) toast(t('categorization.noMatches'));
      return succeeded;
    },
    [queryClient, t, tc]
  );
}
