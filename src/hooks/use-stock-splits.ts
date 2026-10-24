import { useQuery } from '@tanstack/react-query';
import { stockSplitsApi } from '@/lib/tauri-api';

/**
 * The splits of one ticker that still have transactions dated before them (D6). The query is
 * invalidated by the split mutations and, through the `['stock-splits']` prefix, by the stock
 * domain refresh.
 */
export function useStockSplits(ticker: string | undefined) {
  return useQuery({
    queryKey: ['stock-splits', ticker],
    queryFn: () => stockSplitsApi.list(ticker as string),
    enabled: !!ticker,
  });
}
