import { useCallback, useSyncExternalStore } from 'react';
import { useQueryClient, type QueryClient } from '@tanstack/react-query';
import { failedLoad } from '@/mobile/lib/load-state';

/**
 * Whether a query the screen is showing (an active one) has failed without an earlier answer
 * (`failedLoad`, audit C6). A record missing because its query failed is not "not found": the detail pages ask
 * this before they say so.
 */
export function useActiveQueryFailed(): boolean {
  const client = useQueryClient();
  const cache = client.getQueryCache();
  return useSyncExternalStore(
    useCallback((listener) => cache.subscribe(listener), [cache]),
    () => cache.findAll({ type: 'active' }).some((query) => failedLoad(query.state))
  );
}

/** "Zkusit znovu": loads again every query on the screen that failed. */
export function retryFailedQueries(client: QueryClient): Promise<void> {
  return client.refetchQueries({
    type: 'active',
    predicate: (query) => query.state.status === 'error',
  });
}
