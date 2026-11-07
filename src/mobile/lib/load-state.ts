import { ApiError } from '@/lib/api-error';

/**
 * What a phone screen shows for the data it waits on (audit C6): a skeleton while the first load
 * runs, the screen once there is something to show, "could not be loaded" with a retry when the
 * load failed, and "not found" only when the load succeeded without the record. A failed refetch
 * keeps the last answer on screen: an error with data is still `ready`.
 */
export type LoadState = 'loading' | 'ready' | 'error' | 'notFound';

export function loadState(input: {
  isLoading: boolean;
  isError: boolean;
  hasData: boolean;
}): LoadState {
  if (input.hasData) return 'ready';
  if (input.isLoading) return 'loading';
  return input.isError ? 'error' : 'notFound';
}

/**
 * Whether a query's state is a failed load to say so about: it failed and holds no earlier answer,
 * and the failure is not the backend's "not found" (that is the record missing, said as such).
 */
export function failedLoad(state: { status: string; data: unknown; error: unknown }): boolean {
  if (state.status !== 'error' || state.data !== undefined) return false;
  return !(state.error instanceof ApiError && state.error.kind === 'notFound');
}
