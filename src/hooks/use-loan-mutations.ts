import { useMutation, useQueryClient } from '@tanstack/react-query';
import { loansApi, portfolioApi } from '@/lib/tauri-api';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import { translateApiError } from '@/lib/translate-api-error';
import type { InsertLoan } from '@shared/schema';

/**
 * Mutation contract (docs/standards/typescript-frontend.md): domain keys, net worth
 * and cashflow, then a fresh snapshot for the trend charts. A loan also feeds the
 * schedule, the real-estate equity (linked / available loans), the projection start
 * and its own event log.
 */
export function useLoanRefresh() {
  const queryClient = useQueryClient();
  return async (loanId?: string) => {
    queryClient.invalidateQueries({ queryKey: ['loans'] });
    queryClient.invalidateQueries({ queryKey: ['loan-schedule'] });
    if (loanId) queryClient.invalidateQueries({ queryKey: ['loan-events', loanId] });
    queryClient.invalidateQueries({ queryKey: ['available-loans'] });
    queryClient.invalidateQueries({ queryKey: ['real-estate-loans'] });
    queryClient.invalidateQueries({ queryKey: ['projection'] });
    queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
    // The write already succeeded; a failed snapshot must not turn it into an error.
    try {
      await portfolioApi.recordSnapshot();
      queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
    } catch (error) {
      console.error('Failed to record portfolio snapshot:', error);
    }
  };
}

export function useLoanMutations() {
  const refreshAfterChange = useLoanRefresh();
  const { t } = useTranslation('common');
  const { t: tl } = useTranslation('loans');

  const onError = (error: Error) => {
    toast.error(t('status.error'), { description: translateApiError(error, t) });
  };

  const createMutation = useMutation({
    mutationFn: (data: InsertLoan) => loansApi.create(data),
    onSuccess: async () => {
      await refreshAfterChange();
      toast(tl('toast.added'));
    },
    onError,
  });

  const updateMutation = useMutation({
    mutationFn: ({ id, ...data }: { id: string } & Partial<InsertLoan>) => {
      return loansApi.update(id, data);
    },
    onSuccess: async (_loan, variables) => {
      await refreshAfterChange(variables.id);
      toast(tl('toast.updated'));
    },
    onError,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => loansApi.delete(id),
    onSuccess: async () => {
      await refreshAfterChange();
      toast(tl('toast.deleted'));
    },
    onError,
  });

  return {
    createMutation,
    updateMutation,
    deleteMutation,
  };
}
