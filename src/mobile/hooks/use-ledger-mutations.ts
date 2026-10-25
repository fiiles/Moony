import { useMutation, useQueryClient, type UseMutationResult } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { FlowKind } from '@shared/schema';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { bankAccountsApi, categorizationApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

export interface LedgerMutations {
  /** No toast of its own: the category picker says what it did. */
  setCategory: UseMutationResult<void, Error, { id: string; categoryId: string | null }>;
  /** `null` returns the row to the automatic meaning. */
  setFlowKind: UseMutationResult<void, Error, { id: string; flowKind: FlowKind | null }>;
  deleteTransaction: UseMutationResult<void, Error, string>;
  /** Accepts a pending MCP suggestion on this one row (nothing is learned). */
  acceptSuggestion: UseMutationResult<void, Error, string>;
  declineSuggestion: UseMutationResult<void, Error, string>;
}

/**
 * The ledger's writes on one transaction (spec §8.4): category, meaning, delete and the MCP
 * suggestion. Each refreshes the money flows (frontend standard rule 3: the domain keys, the
 * snapshot and the history) and says what it did; a failure is a toast with the translated error.
 * `quiet` leaves the "what it did" toasts out, for a screen that shows the result itself (the
 * categorize queue turns the row over): errors are still said.
 */
export function useLedgerMutations({ quiet = false }: { quiet?: boolean } = {}): LedgerMutations {
  const queryClient = useQueryClient();
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');

  const done = async (message?: string) => {
    await refreshMoneyFlows(queryClient);
    if (message && !quiet) toast(message);
  };
  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const setCategory = useMutation({
    mutationFn: ({ id, categoryId }: { id: string; categoryId: string | null }) =>
      bankAccountsApi.updateTransactionCategory(id, categoryId),
    onSuccess: () => done(),
    onError,
  });

  const setFlowKind = useMutation({
    mutationFn: ({ id, flowKind }: { id: string; flowKind: FlowKind | null }) =>
      bankAccountsApi.updateTransactionFlow(id, flowKind),
    onSuccess: () => done(t('transaction.flowSaved')),
    onError,
  });

  const deleteTransaction = useMutation({
    mutationFn: (id: string) => bankAccountsApi.deleteTransaction(id),
    onSuccess: () => done(t('bank_accounts:messages.transactionDeleted')),
    onError,
  });

  const acceptSuggestion = useMutation({
    mutationFn: async (id: string) => {
      await categorizationApi.acceptSuggestion(id);
    },
    onSuccess: () => done(t('bank_accounts:categorization.suggestionAccepted')),
    onError,
  });

  const declineSuggestion = useMutation({
    mutationFn: (id: string) => categorizationApi.declineSuggestion(id),
    onSuccess: () => done(),
    onError,
  });

  return { setCategory, setFlowKind, deleteTransaction, acceptSuggestion, declineSuggestion };
}
