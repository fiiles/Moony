import { QueryClient, QueryObserver } from '@tanstack/react-query';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { invalidateAfterPriceRefresh } from './price-refresh';

const clients: QueryClient[] = [];
afterEach(() => clients.splice(0).forEach((client) => client.clear()));
function client() {
  const result = new QueryClient({
    defaultOptions: { queries: { staleTime: Infinity, retry: false } },
  });
  clients.push(result);
  return result;
}

describe('invalidateAfterPriceRefresh', () => {
  it('refetches a shown position', async () => {
    const cache = client();
    const key = ['investment', 'p1', 'CZK', 0];
    const queryFn = vi.fn(async () => ({ id: 'p1' }));
    await cache.fetchQuery({ queryKey: key, queryFn });
    const stop = new QueryObserver(cache, { queryKey: key, queryFn }).subscribe(() => undefined);

    await invalidateAfterPriceRefresh(cache);

    expect(queryFn).toHaveBeenCalledTimes(2);
    stop();
  });

  // `delete_investment` emits `recalculation-complete` before it returns, and SyncProvider answers
  // it with this refresh: the phone's position page pauses its queries while its delete runs
  // (`usePosition(…, live)`), so the deleted id is never asked for again and the data it shows stays.
  it('leaves a paused position alone, its data kept', async () => {
    const cache = client();
    const key = ['investment', 'p1', 'CZK', 0];
    const queryFn = vi.fn(async () => ({ id: 'p1' }));
    await cache.fetchQuery({ queryKey: key, queryFn });
    const stop = new QueryObserver(cache, { queryKey: key, queryFn, enabled: false }).subscribe(
      () => undefined
    );

    await invalidateAfterPriceRefresh(cache);

    expect(queryFn).toHaveBeenCalledTimes(1);
    expect(cache.getQueryData(key)).toEqual({ id: 'p1' });
    stop();
  });
});
