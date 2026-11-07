import { useCallback } from 'react';
import { useQueryClient } from '@tanstack/react-query';

/**
 * The pull to refresh of a screen that shows what is stored (Přehled, Finance, Majetek): every
 * query the screen is showing is read again. The data is local, so nothing is fetched from the
 * network; a pull picks up what a write elsewhere (the desktop through device sync, an import
 * from the share sheet) has changed. Resolves when they have all answered.
 */
export function useRefetchShown(): () => Promise<void> {
  const queryClient = useQueryClient();
  return useCallback(() => queryClient.refetchQueries({ type: 'active' }), [queryClient]);
}
