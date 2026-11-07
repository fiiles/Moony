import { useCallback } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { refetchedByPull } from '@/mobile/lib/refetch-scope';

/**
 * The pull to refresh of a screen that shows what is stored (Přehled, Finance, Majetek): the
 * queries the screen is showing (the active ones: those of the screen and its always-mounted
 * parts, such as Údržba) are read again, except those that reach the network (`refetch-scope.ts`).
 * The data is local; a pull picks up what a write elsewhere (the desktop through device sync, an
 * import from the share sheet) has changed. Resolves when they have all answered.
 */
export function useRefetchShown(): () => Promise<void> {
  const queryClient = useQueryClient();
  return useCallback(
    () =>
      queryClient.refetchQueries({
        type: 'active',
        predicate: (query) => refetchedByPull(query.queryKey),
      }),
    [queryClient]
  );
}
