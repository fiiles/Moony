import { QueryClient } from '@tanstack/react-query';

/**
 * Query Client for Tauri
 *
 * Unlike the Express.js version, we don't use a default queryFn
 * because Tauri invoke calls are not URL-based.
 * Each hook will explicitly call the Tauri API.
 */
export const queryClient = new QueryClient({
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
