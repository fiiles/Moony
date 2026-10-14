import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { budgetingApi, type InsertBudgetGoal } from '@/lib/tauri-api';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { translateApiError } from '@/lib/translate-api-error';
import type { BudgetGoalInterval } from '@shared/schema';

export function useSaveBudgetGoal(onSaved: () => void, editing: boolean) {
  const client = useQueryClient();
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  return useMutation({
    mutationFn: (data: InsertBudgetGoal) => budgetingApi.upsertBudgetGoal(data),
    onSuccess: async () => {
      await refreshMoneyFlows(client);
      toast.success(t(editing ? 'dialog.updated' : 'dialog.added'));
      onSaved();
    },
    onError: (error: Error) => toast.error(translateApiError(error, tc)),
  });
}

export function useDeleteBudgetIntervals(onDeleted: () => void) {
  const client = useQueryClient();
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  return useMutation({
    mutationFn: (intervals: BudgetGoalInterval[]) =>
      budgetingApi.deleteBudgetGoalIntervals(intervals),
    onSuccess: async () => {
      await refreshMoneyFlows(client);
      toast.success(t('manage.intervalDeleted'));
      onDeleted();
    },
    onError: (error: Error) => toast.error(translateApiError(error, tc)),
  });
}

/** Delete a category's budget with every limit; past reports lose it too. */
export function useDeleteBudget(onDeleted: () => void) {
  const client = useQueryClient();
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  return useMutation({
    mutationFn: ({ categoryId, timeframe }: { categoryId: string; timeframe: string }) =>
      budgetingApi.deleteBudgetGoal(categoryId, timeframe),
    onSuccess: async () => {
      await refreshMoneyFlows(client);
      toast.success(t('manage.deletedToast'));
      onDeleted();
    },
    onError: (error: Error) => toast.error(translateApiError(error, tc)),
  });
}
