import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { bankAccountsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type { InsertTransactionCategory, UpdateTransactionCategory } from '@shared/schema';

export function useCategoryMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');

  const onError = (error: Error) => {
    toast.error(t('messages.error'), { description: translateApiError(error, tc) });
  };

  const createCategory = useMutation({
    mutationFn: (data: InsertTransactionCategory) => bankAccountsApi.createCategory(data),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('messages.categoryCreated'));
    },
    onError,
  });

  const updateCategory = useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateTransactionCategory }) =>
      bankAccountsApi.updateCategoryDefinition(id, data),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('messages.categoryUpdated'));
    },
    onError,
  });

  const deleteCategory = useMutation({
    mutationFn: ({ id, reassignTo }: { id: string; reassignTo?: string | null }) =>
      bankAccountsApi.deleteCategory(id, reassignTo),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('messages.categoryDeleted'));
    },
    onError,
  });

  return { createCategory, updateCategory, deleteCategory };
}
