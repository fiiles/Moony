import { useQuery } from '@tanstack/react-query';
import { bondsApi } from '@/lib/tauri-api';

/**
 * Whether the bonds query has failed. `useBonds()` (desktop, reuse-only) returns no `isError`, so
 * a failed load would read as "no bonds yet"; this is a second observer on the same key and
 * query function — the cache and the request are shared, nothing is fetched twice — whose only
 * job is to say so. A refetch that fails after data came in leaves `useBonds().bonds` as it was,
 * so the page keeps showing that data and consults this flag only when there is none.
 */
export function useBondsLoadFailed(): boolean {
  const { isError } = useQuery({ queryKey: ['bonds'], queryFn: () => bondsApi.getAll() });
  return isError;
}
