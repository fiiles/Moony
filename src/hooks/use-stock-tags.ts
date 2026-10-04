import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { stockTagsApi } from '@/lib/tauri-api';
import type { StockInvestmentWithTags, StockTag } from '@shared/schema';

const NO_TAGS: StockTag[] = [];

/**
 * The tags of every stock position, keyed by investment id. Shares the `["stocks-analysis"]`
 * query with the Stocks Analysis page, so tag edits there (and investment mutations) refresh it.
 */
export function useStockTagsByInvestment(): ReadonlyMap<string, StockTag[]> {
  const { data } = useQuery<StockInvestmentWithTags[]>({
    queryKey: ['stocks-analysis'],
    queryFn: stockTagsApi.getAnalysis,
  });

  return useMemo(
    () => new Map((data ?? []).map((stock) => [stock.id, stock.tags ?? NO_TAGS])),
    [data]
  );
}
