import { useCallback, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification';

/** What the system says about notifications; `unknown` where the plugin does not answer. */
export type NotificationPermissionState = 'granted' | 'denied' | 'default' | 'unknown';

/** What the plugin can answer: the cache holds this, `unknown` is the absence of an answer. */
type PermissionAnswer = Exclude<NotificationPermissionState, 'unknown'>;

async function checkPermission(): Promise<PermissionAnswer> {
  // The plugin answers `null` until the person has been asked; its typing says `boolean`
  const granted: boolean | null = await isPermissionGranted();
  if (granted === null) return 'default';
  return granted ? 'granted' : 'denied';
}

/**
 * Whether the phone lets Moony notify (Nastavení → Oznámení). `request` is the one place that
 * asks, from the button on that page and never on its own; its answer goes into the cache the
 * query reads, so the root of Nastavení and the scheduler see it at once. A call the shell cannot
 * answer (a desktop shell or a browser without the plugin) rejects and reads as `unknown`.
 * `isPending` is true while the state is not known yet and while a request is open.
 */
export function useNotificationPermission(): {
  state: NotificationPermissionState;
  request: () => Promise<void>;
  isPending: boolean;
} {
  const queryClient = useQueryClient();
  const [requesting, setRequesting] = useState(false);
  const query = useQuery({
    queryKey: ['notification-permission'],
    queryFn: checkPermission,
    // The person may have changed it in the phone's settings since this page was last open
    refetchOnMount: 'always',
  });

  const request = useCallback(async () => {
    setRequesting(true);
    try {
      queryClient.setQueryData<PermissionAnswer>(
        ['notification-permission'],
        await requestPermission()
      );
    } catch (error) {
      console.warn('[notifications] the permission request failed', error);
    } finally {
      setRequesting(false);
    }
  }, [queryClient]);

  return {
    state: query.data ?? 'unknown',
    request,
    isPending: query.isPending || requesting,
  };
}
