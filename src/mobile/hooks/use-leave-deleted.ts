import { useQueryClient, type QueryKey } from '@tanstack/react-query';
import { useLocation } from 'wouter';
import { useBack } from '@/mobile/hooks/use-back';

/** How long after leaving a deleted position the screen may still be it (a restored deep link). */
const DELETED_GUARD_MS = 250;

/**
 * What a detail does once its record is deleted: back to the list (`listRoute`) as Back does, so
 * the history keeps no ghost of it. Its own queries (`queryKeys`) are dropped once the screen has
 * left (history.back lands asynchronously): removed while it is still up, a re-render would
 * rebuild them empty and flash the skeleton. On a restored deep link the detail is the first
 * entry and back has nowhere to go; the screen is then replaced. `detailPath` is the screen's own
 * path, so a screen that has meanwhile become another one is left alone.
 */
export function useLeaveDeleted(
  listRoute: string
): (detailPath: string, queryKeys: readonly QueryKey[]) => void {
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const back = useBack(listRoute);
  return (detailPath, queryKeys) => {
    back();
    window.setTimeout(() => {
      if (window.location.pathname === detailPath) {
        setLocation(listRoute, { replace: true });
      }
      for (const queryKey of queryKeys) queryClient.removeQueries({ queryKey });
    }, DELETED_GUARD_MS);
  };
}
