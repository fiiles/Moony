import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { InsertLoanEvent, LoanEvent } from '@shared/schema';
import { loansApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useLoanRefresh } from '@/hooks/use-loan-mutations';

/** Extra payments, rate changes and balance checks of one loan, oldest first (migration 014). */
export function useLoanEvents(loanId: string | undefined) {
  return useQuery<LoanEvent[]>({
    queryKey: ['loan-events', loanId],
    queryFn: () => loansApi.getEvents(loanId!),
    enabled: !!loanId,
  });
}

/**
 * Record or remove a loan event. Adding one re-anchors the amortization on its
 * day (the backend applies the side effect), so the whole loan refresh runs.
 */
export function useLoanEventMutations(loanId: string | undefined) {
  const refresh = useLoanRefresh();
  const { t } = useTranslation('common');
  const { t: tl } = useTranslation('loans');
  const onError = (error: Error) => {
    toast.error(t('status.error'), { description: translateApiError(error, t) });
  };

  const add = useMutation({
    mutationFn: (data: InsertLoanEvent) => loansApi.addEvent(data),
    onSuccess: async () => {
      await refresh(loanId);
      toast(tl('toast.eventAdded'));
    },
    onError,
  });

  const remove = useMutation({
    mutationFn: (eventId: string) => loansApi.deleteEvent(eventId),
    onSuccess: async () => {
      await refresh(loanId);
      toast(tl('toast.eventDeleted'));
    },
    onError,
  });

  return { add, remove };
}
