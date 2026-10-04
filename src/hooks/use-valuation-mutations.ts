import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { InsertAssetValuation } from '@shared/schema';
import { otherAssetsApi, portfolioApi, realEstateApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

export type ValuationKind = 'realEstate' | 'otherAsset';

/** Query key of one asset's valuation log. */
export function valuationsKey(kind: ValuationKind, assetId: string) {
  return kind === 'realEstate'
    ? (['real-estate-valuations', assetId] as const)
    : (['other-asset-valuations', assetId] as const);
}

/**
 * Record or remove a dated estimate (migration 014). Follows the mutation
 * contract: domain keys + portfolio-metrics + cashflow-report, then a fresh
 * snapshot and portfolio-history (reference: use-bank-account-mutations.ts).
 */
export function useValuationMutations(kind: ValuationKind, assetId: string | undefined) {
  const queryClient = useQueryClient();
  const { t } = useTranslation('common');

  const refresh = async () => {
    if (assetId) queryClient.invalidateQueries({ queryKey: valuationsKey(kind, assetId) });
    if (kind === 'realEstate') {
      queryClient.invalidateQueries({ queryKey: ['real-estate'] });
    } else {
      queryClient.invalidateQueries({ queryKey: ['other-assets'] });
    }
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
  const onError = (error: Error) => {
    toast.error(t('status.error'), { description: translateApiError(error, t) });
  };

  const add = useMutation({
    mutationFn: (data: InsertAssetValuation) =>
      kind === 'realEstate' ? realEstateApi.addValuation(data) : otherAssetsApi.addValuation(data),
    onSuccess: async () => {
      await refresh();
      toast(t('revalue.saved'));
    },
    onError,
  });

  const remove = useMutation({
    mutationFn: (valuationId: string) =>
      kind === 'realEstate'
        ? realEstateApi.deleteValuation(valuationId)
        : otherAssetsApi.deleteValuation(valuationId),
    onSuccess: async () => {
      await refresh();
      toast(t('revalue.removed'));
    },
    onError,
  });

  return { add, remove };
}
