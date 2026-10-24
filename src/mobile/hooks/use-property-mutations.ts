import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { ALL_REAL_ESTATE_VALUATIONS_KEY } from '@/hooks/use-real-estate';
import { portfolioApi, realEstateApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

/** What a link or an unlink acts on: a loan or a policy of the property. */
export interface LinkTarget {
  kind: 'loan' | 'insurance';
  itemId: string;
}

/**
 * The property's writes: delete, and link or unlink a loan or a policy. The caller leaves the
 * screen when a delete has succeeded and then drops the property's own queries
 * (`PropertyDetail`, `useDeleteAndLeave`: removing them while the screen is still up would rebuild
 * them empty).
 *
 * A delete follows the mutation contract (the domain keys, the metrics and the cashflow, then a
 * fresh snapshot and the history) and frees the loans and policies that were linked. A link or an
 * unlink refreshes what the desktop detail does (`invalidateLinks`): the property's own lists, the
 * lists of what can still be linked, the loans and policies, and the cashflow report.
 */
export function usePropertyMutations(id: string | undefined) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  /** What a change of the links touches outside the property's own lists. */
  const invalidateLinked = () => {
    queryClient.invalidateQueries({ queryKey: ['available-insurances'] });
    queryClient.invalidateQueries({ queryKey: ['available-loans'] });
    queryClient.invalidateQueries({ queryKey: ['loans'] });
    queryClient.invalidateQueries({ queryKey: ['insurance'] });
    queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
  };
  const invalidateLinks = () => {
    queryClient.invalidateQueries({ queryKey: ['real-estate-insurances', id] });
    queryClient.invalidateQueries({ queryKey: ['real-estate-loans', id] });
    invalidateLinked();
  };

  const remove = useMutation({
    mutationFn: () => realEstateApi.delete(id!),
    onSuccess: async () => {
      queryClient.invalidateQueries({ queryKey: ['real-estate'] });
      queryClient.invalidateQueries({ queryKey: ['real-estate', id] });
      queryClient.invalidateQueries({ queryKey: ['real-estate-valuations', id] });
      queryClient.invalidateQueries({ queryKey: ALL_REAL_ESTATE_VALUATIONS_KEY });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      // Its loans and policies are no longer linked to anything
      invalidateLinked();
      // The delete already succeeded; a failed snapshot must not turn it into an error.
      try {
        await portfolioApi.recordSnapshot();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (error) {
        console.error('Failed to record portfolio snapshot:', error);
      }
      toast(t('toast.deleted'));
    },
    onError,
  });

  const link = useMutation({
    mutationFn: ({ kind, itemId }: LinkTarget) =>
      kind === 'loan'
        ? realEstateApi.linkLoan(id!, itemId)
        : realEstateApi.linkInsurance(id!, itemId),
    onSuccess: (_, { kind }) => {
      invalidateLinks();
      toast(t(kind === 'loan' ? 'toast.loanLinked' : 'toast.insuranceLinked'));
    },
    onError,
  });

  const unlink = useMutation({
    mutationFn: ({ kind, itemId }: LinkTarget) =>
      kind === 'loan'
        ? realEstateApi.unlinkLoan(id!, itemId)
        : realEstateApi.unlinkInsurance(id!, itemId),
    onSuccess: (_, { kind }) => {
      invalidateLinks();
      toast(t(kind === 'loan' ? 'toast.loanUnlinked' : 'toast.insuranceUnlinked'));
    },
    onError,
  });

  return { remove, link, unlink };
}
