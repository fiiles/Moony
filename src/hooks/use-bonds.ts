import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { todayUtcDay } from '@shared/calculations';
import { bondsApi } from '@/lib/tauri-api';
import { useMainConvert } from '@/hooks/use-fx';
import { bondsSummary, type BondsSummary } from '@/utils/bonds';

export type BondsMetrics = BondsSummary;

/**
 * Bonds with the list page's derived numbers (design system §7 List, prototype bonds.html),
 * in the main currency at the current quotes.
 */
export function useBonds() {
  const { data: bonds = [], isLoading } = useQuery({
    queryKey: ['bonds'],
    queryFn: () => bondsApi.getAll(),
  });
  const today = todayUtcDay();
  const convert = useMainConvert();

  const metrics = useMemo(() => bondsSummary(bonds, convert, today), [bonds, convert, today]);

  return { bonds, isLoading, metrics, today, convert };
}
