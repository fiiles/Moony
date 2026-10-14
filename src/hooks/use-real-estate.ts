import { useQuery } from '@tanstack/react-query';
import type { AssetValuation } from '@shared/schema';
import { realEstateApi } from '@/lib/tauri-api';

/**
 * Query key of the valuations of every property at once (the list chart's revaluation marks).
 * It sits under `['real-estate-valuations']`, so the per-property keys and the sync event of
 * the real estate domain reach it by prefix; a write to one property's log invalidates it
 * explicitly.
 */
export const ALL_REAL_ESTATE_VALUATIONS_KEY = ['real-estate-valuations', 'all'] as const;

/** Every dated valuation of every property, oldest first (each row names its property). */
export function useAllRealEstateValuations() {
  return useQuery<AssetValuation[]>({
    queryKey: ALL_REAL_ESTATE_VALUATIONS_KEY,
    queryFn: () => realEstateApi.getAllValuations(),
  });
}
