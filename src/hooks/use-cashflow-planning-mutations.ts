import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { InsertCashflowItem, SetCashflowPlanInclusion } from '@shared/schema';
import { cashflowApi } from '@/lib/tauri-api';
import { refreshMoneyFlows } from '@/lib/money-flow-refresh';
import { translateApiError } from '@/lib/translate-api-error';

export type PlanInclusionChange = SetCashflowPlanInclusion & { kind: 'item' | 'category' };

/** Planning item create / update / delete and plan inclusion (rule 7 through `refreshMoneyFlows`). */
export function useCashflowPlanningMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');

  const onError = (error: Error) =>
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });

  const createMutation = useMutation({
    mutationFn: (data: InsertCashflowItem) => cashflowApi.createItem(data),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('planning.toast.added'));
    },
    onError,
  });

  const updateMutation = useMutation({
    mutationFn: ({ id, data }: { id: string; data: InsertCashflowItem }) =>
      cashflowApi.updateItem(id, data),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('planning.toast.updated'));
    },
    onError,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => cashflowApi.deleteItem(id),
    onSuccess: async () => {
      await refreshMoneyFlows(queryClient);
      toast(t('planning.toast.deleted'));
    },
    onError,
  });

  const inclusionMutation = useMutation({
    mutationFn: ({ key, included }: PlanInclusionChange) =>
      cashflowApi.setInclusion({ key, included }),
    onSuccess: async (_result, { kind, included }) => {
      await refreshMoneyFlows(queryClient);
      toast(
        t(
          kind === 'category'
            ? included
              ? 'planning.toast.categoryIncluded'
              : 'planning.toast.categoryExcluded'
            : included
              ? 'planning.toast.included'
              : 'planning.toast.excluded'
        )
      );
    },
    onError,
  });

  return { createMutation, updateMutation, deleteMutation, inclusionMutation };
}
