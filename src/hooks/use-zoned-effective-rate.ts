import { useQuery } from '@tanstack/react-query';
import { bankAccountsApi } from '@/lib/tauri-api';
import { calculateEffectiveRate } from '@/utils/bank-account-zones';

/**
 * Hook to get effective rate for a zoned account
 */
export function useZonedEffectiveRate(accountId: string, balance: number, enabled: boolean) {
  const { data: zones } = useQuery({
    queryKey: ['bank-account-zones', accountId],
    queryFn: () => bankAccountsApi.getZones(accountId),
    enabled,
    staleTime: 5 * 60 * 1000, // 5 minutes
  });

  const effectiveRate = zones ? calculateEffectiveRate(balance, zones) : 0;

  return { effectiveRate, zones };
}
