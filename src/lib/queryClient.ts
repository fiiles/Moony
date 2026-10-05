import { MutationCache, QueryClient } from '@tanstack/react-query';

// Any successful write can add, move or resolve a milestone (an edited policy, a new
// target, a recorded loan event), so every mutation refreshes ["milestones"].
const mutationCache = new MutationCache({
  onSuccess: () => {
    void queryClient.invalidateQueries({ queryKey: ['milestones'] });
  },
});

/**
 * Query Client for Tauri
 *
 * Unlike the Express.js version, we don't use a default queryFn
 * because Tauri invoke calls are not URL-based.
 * Each hook will explicitly call the Tauri API.
 */
export const queryClient = new QueryClient({
  mutationCache,
  defaultOptions: {
    queries: {
      refetchInterval: false,
      refetchOnWindowFocus: false,
      staleTime: Infinity,
      retry: false,
    },
    mutations: {
      retry: false,
    },
  },
});
