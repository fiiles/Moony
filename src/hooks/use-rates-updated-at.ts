import { useQuery } from '@tanstack/react-query';
import { portfolioApi } from '@/lib/tauri-api';

/**
 * When the exchange rates were last refreshed (unix seconds), or null when
 * unknown. Derived from the price-status query (shared with the stale-prices
 * indicator): the backend reports the rates' age in whole hours, so the result
 * is accurate to about an hour — enough for a "rates of 2 Oct" tooltip.
 */
export function useRatesUpdatedAt(): number | null {
  const { data, dataUpdatedAt } = useQuery({
    queryKey: ['price-status'],
    queryFn: () => portfolioApi.getPriceStatus(),
    staleTime: 30000,
  });
  const ageHours = data?.exchangeRatesAgeHours;
  if (ageHours === null || ageHours === undefined || !dataUpdatedAt) return null;
  return Math.floor(dataUpdatedAt / 1000) - ageHours * 3600;
}
