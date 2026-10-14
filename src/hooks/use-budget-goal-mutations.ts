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

/** End a goal from the current month: earlier limits stay in the reports, scheduled changes go. */
export function useEndBudgetGoal(onEnded: () => void) {
  const client = useQueryClient();
  const { t } = useTranslation('budgeting');
  const { t: tc } = useTranslation('common');
  return useMutation({
    mutationFn: (id: string) => budgetingApi.deleteBudgetGoal(id),
    onSuccess: async () => {
      await refreshMoneyFlows(client);
      toast.success(t('manage.endedToast'));
      onEnded();
    },
    onError: (error: Error) => toast.error(translateApiError(error, tc)),
  });
}
