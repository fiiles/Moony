import { describe, expect, it } from 'vitest';
import { QueryClient, QueryObserver } from '@tanstack/react-query';
import { dropQuery } from './drop-query';

const KEY = ['device-sync-pairing'] as const;
const VIEW = { stage: 'compare', attemptsLeft: 1 };

/** Lets the query client deliver its batched notifications. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

/** A mounted screen's observer of KEY, showing the view a start or a confirm put there. */
function mountedObserver(client: QueryClient) {
  client.setQueryData(KEY, VIEW);
  const observer = new QueryObserver(client, {
    queryKey: KEY,
    queryFn: () => VIEW,
    enabled: false,
  });
  const seen: unknown[] = [];
  const unsubscribe = observer.subscribe((result) => seen.push(result.data));
  return { observer, seen, unsubscribe };
}

describe('dropQuery', () => {
  it('tells a mounted observer that the data is gone, so the screen leaves the old view', async () => {
    const client = new QueryClient();
    const { observer, seen, unsubscribe } = mountedObserver(client);
    expect(observer.getCurrentResult().data).toEqual(VIEW);

    dropQuery(client, KEY);
    await settle();

    expect(seen).toHaveLength(1);
    expect(seen[0]).toBeUndefined();
    expect(observer.getCurrentResult().data).toBeUndefined();
    expect(observer.getCurrentResult().isError).toBe(false);
    unsubscribe();
  });

  it('removes the entry, so nothing of the old view stays in the cache', async () => {
    const client = new QueryClient();
    const { unsubscribe } = mountedObserver(client);

    dropQuery(client, KEY);
    await settle();

    expect(client.getQueryData(KEY)).toBeUndefined();
    expect(client.getQueryCache().find({ queryKey: KEY, exact: true })).toBeUndefined();
    unsubscribe();
  });

  it('does nothing when there is no such query', () => {
    const client = new QueryClient();
    expect(() => dropQuery(client, KEY)).not.toThrow();
    expect(client.getQueryData(KEY)).toBeUndefined();
  });

  it('leaves other queries alone', () => {
    const client = new QueryClient();
    client.setQueryData(KEY, VIEW);
    client.setQueryData([...KEY, 'other'], 'kept');
    client.setQueryData(['device-sync-status'], 'kept');

    dropQuery(client, KEY);

    expect(client.getQueryData([...KEY, 'other'])).toBe('kept');
    expect(client.getQueryData(['device-sync-status'])).toBe('kept');
  });
});
