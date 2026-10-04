import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { InsertBond } from '@shared/schema';
import { bondsApi, portfolioApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

/** Bond create / update / delete following the mutation contract (rule 7). */
export function useBondMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('bonds');
  const { t: tc } = useTranslation('common');

  const refresh = async () => {
    queryClient.invalidateQueries({ queryKey: ['bonds'] });
    queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
    queryClient.invalidateQueries({ queryKey: ['projection'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
    // The write already succeeded; a failed snapshot must not turn it into an error.
    try {
      await portfolioApi.recordSnapshot();
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
    } catch (error) {
      console.error('Failed to record portfolio snapshot:', error);
    }
  };
  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const createMutation = useMutation({
    mutationFn: (data: InsertBond) => bondsApi.create(data),
    onSuccess: async () => {
      await refresh();
      toast(t('toast.added'));
    },
    onError,
  });

  const updateMutation = useMutation({
    mutationFn: ({ id, ...data }: { id: string } & InsertBond) => bondsApi.update(id, data),
    onSuccess: async () => {
      await refresh();
      toast(t('toast.updated'));
    },
    onError,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => bondsApi.delete(id),
    onSuccess: async () => {
      await refresh();
      toast(t('toast.deleted'));
    },
    onError,
  });

  return { createMutation, updateMutation, deleteMutation };
}
