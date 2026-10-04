import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { portfolioApi, realEstateApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import type { RealEstate, RecurringCost } from '@shared/schema';
import { realEstateUpdatePayload } from '@/utils/real-estate-payload';

/**
 * Real-estate writes that do not go through the full property form.
 * Follows the mutation contract: domain keys + portfolio-metrics + cashflow-report,
 * then a fresh snapshot and portfolio-history (reference: use-bank-account-mutations.ts).
 * A property save can write a valuation (a changed market price), so the property's
 * valuation log, which the detail chart reads, is refreshed with it.
 */
export function useRealEstateMutations() {
  const queryClient = useQueryClient();
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');

  /**
   * Append one recurring cost to a property. The backend update replaces the whole
   * property (rent and notes are not merged), so every field is sent back unchanged.
   */
  const addRecurringCost = useMutation({
    mutationFn: ({ realEstate, cost }: { realEstate: RealEstate; cost: RecurringCost }) =>
      realEstateApi.update(
        realEstate.id,
        realEstateUpdatePayload(realEstate, {
          recurringCosts: [...(realEstate.recurringCosts ?? []), cost],
        })
      ),
    onSuccess: async (_data, { realEstate }) => {
      queryClient.invalidateQueries({ queryKey: ['real-estate'] });
      queryClient.invalidateQueries({ queryKey: ['real-estate', realEstate.id] });
      queryClient.invalidateQueries({ queryKey: ['real-estate-valuations', realEstate.id] });
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['cashflow-report'] });
      // The update already succeeded; a failed snapshot must not turn it into an error.
      try {
        await portfolioApi.recordSnapshot();
        queryClient.invalidateQueries({ queryKey: ['portfolio-history'] });
      } catch (error) {
        console.error('Failed to record portfolio snapshot:', error);
      }
      toast(tc('status.success'), { description: t('toast.recurringCostAdded') });
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  /**
   * Save the free-text notes of a property (RED-06). Notes carry no money, so only the
   * property queries are refreshed: no portfolio snapshot or net-worth invalidation.
   */
  const updateNotes = useMutation({
    mutationFn: ({ realEstate, notes }: { realEstate: RealEstate; notes: string }) =>
      realEstateApi.update(realEstate.id, realEstateUpdatePayload(realEstate, { notes })),
    onSuccess: (_data, { realEstate }) => {
      queryClient.invalidateQueries({ queryKey: ['real-estate'] });
      queryClient.invalidateQueries({ queryKey: ['real-estate', realEstate.id] });
      queryClient.invalidateQueries({ queryKey: ['real-estate-valuations', realEstate.id] });
      toast(tc('status.success'), { description: t('toast.notesSaved') });
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  return { addRecurringCost, updateNotes };
}
