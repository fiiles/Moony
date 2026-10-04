import { useMutation, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { bankAccountsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type { InsertTransactionCategory, UpdateTransactionCategory } from '@shared/schema';

/**
 * Everything a category change can alter: the category list and usage counts,
 * transactions (names, reassignment), the budgeting and cashflow reports, and
 * the rules / learned payees / budget goals a delete moves to another
 * category. Categories do not change any portfolio value, so no snapshot is
 * recorded (rule 7 only applies to value-changing mutations).
 */
function invalidateCategoryDomain(queryClient: QueryClient) {
  const keys = [
    'transaction-categories',
    'transaction-category-usage',
    'bank-transactions',
    'budgeting-report',
    'cashflow-report',
    'budget-goals',
    'learnedPayees',
    'customRules',
  ];
  for (const key of keys) {
    queryClient.invalidateQueries({ queryKey: [key] });
  }
}

export function useCategoryMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');

  const onError = (error: Error) => {
    toast.error(t('messages.error'), { description: translateApiError(error, tc) });
  };

  const createCategory = useMutation({
    mutationFn: (data: InsertTransactionCategory) => bankAccountsApi.createCategory(data),
    onSuccess: () => {
      invalidateCategoryDomain(queryClient);
      toast(t('messages.categoryCreated'));
    },
    onError,
  });

  const updateCategory = useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateTransactionCategory }) =>
      bankAccountsApi.updateCategoryDefinition(id, data),
    onSuccess: () => {
      invalidateCategoryDomain(queryClient);
      toast(t('messages.categoryUpdated'));
    },
    onError,
  });

  const deleteCategory = useMutation({
    mutationFn: ({ id, reassignTo }: { id: string; reassignTo?: string | null }) =>
      bankAccountsApi.deleteCategory(id, reassignTo),
    onSuccess: () => {
      invalidateCategoryDomain(queryClient);
      toast(t('messages.categoryDeleted'));
    },
    onError,
  });

  return { createCategory, updateCategory, deleteCategory };
}
