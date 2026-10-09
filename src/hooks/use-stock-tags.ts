import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { stockTagsApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import type { StockTag, StocksAnalysis } from '@shared/schema';

const NO_TAGS: StockTag[] = [];

/**
 * The tags of every stock position, keyed by investment id. Shares the
 * `["stocks-analysis", currency, fxRevision]` query with the Stocks Analysis page, so tag edits
 * there (and investment mutations, which invalidate the `["stocks-analysis"]` prefix) refresh it.
 */
export function useStockTagsByInvestment(): ReadonlyMap<string, StockTag[]> {
  const { currencyCode, fxRevision } = useCurrency();
  const { data } = useQuery<StocksAnalysis>({
    queryKey: ['stocks-analysis', currencyCode, fxRevision],
    queryFn: () => stockTagsApi.getAnalysis(currencyCode),
  });

  return useMemo(
    () => new Map((data?.items ?? []).map((stock) => [stock.id, stock.tags ?? NO_TAGS])),
    [data]
  );
}
