import { useQuery } from '@tanstack/react-query';
import { stockSplitsApi } from '@/lib/tauri-api';

/**
 * The splits of one ticker that still have transactions dated before them (D6). The query
 * client never goes stale on its own, so the `['stock-splits']` prefix is invalidated
 * explicitly: by the split mutations and every stock transaction change
 * (`STOCK_TRANSACTION_KEYS`), by the stock import, and by every price refresh
 * (`invalidateAfterPriceRefresh`, which the daily split check runs inside and which the
 * `recalculation-complete` listener in SyncProvider calls).
 */
export function useStockSplits(ticker: string | undefined, enabled = true) {
  return useQuery({
    queryKey: ['stock-splits', ticker],
    queryFn: () => stockSplitsApi.list(ticker as string),
    // `enabled` false keeps what was read and fetches nothing more (a phone page leaving a deleted position)
    enabled: !!ticker && enabled,
  });
}
