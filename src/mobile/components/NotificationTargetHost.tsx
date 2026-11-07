import { useEffect, useSyncExternalStore } from 'react';
import { onAction } from '@tauri-apps/plugin-notification';
import { useLocation } from 'wouter';
import { useAuth } from '@/hooks/use-auth';
import { notificationPaths, notificationTarget, tapTarget } from '@/mobile/lib/notification-target';
import { routes } from '@/mobile/shell/routes';

/** Only the phone builds have the notification plugin's tap events. */
const hearsTaps = () => {
  const platform: string = import.meta.env.TAURI_ENV_PLATFORM ?? '';
  return platform === 'ios' || platform === 'android';
};

/**
 * Opens the screen of a tapped notification (spec §6, §9; audit C4). Draws nothing; mounted once in
 * the shell and kept through a lock, like `ReceivedFileHost`.
 *
 * A tap (`onAction`, `actionId` "tap") puts its target in `notificationTarget`. While the app is
 * unlocked and not on the lock page the host opens it at once, over the screen shown. While locked
 * it waits: the lock page takes it after a successful unlock (`leaveLock`), so nothing opens
 * behind the lock. Where the listener cannot be registered (a desktop shell, a plugin without the
 * event) the app opens as it would without the tap; a tap that starts the app from cold is not
 * reported by the plugin at all.
 */
export function NotificationTargetHost() {
  const { appStatus, user } = useAuth();
  const [location, setLocation] = useLocation();
  const pending = useSyncExternalStore(notificationTarget.subscribe, notificationTarget.peek);
  const unlocked = appStatus === 'unlocked' && !!user;

  useEffect(() => {
    if (!hearsTaps()) return;
    let current = true;
    let stop: (() => void) | undefined;
    onAction((event) => {
      const path = tapTarget(event, notificationPaths);
      if (path) notificationTarget.offer(path);
    })
      .then((listener) => {
        if (current) stop = () => void listener.unregister();
        else void listener.unregister();
      })
      .catch(() => {
        // No tap events here: a tap opens the app where it was, as before
      });
    return () => {
      current = false;
      stop?.();
    };
  }, []);

  useEffect(() => {
    if (pending === null || !unlocked || location === routes.auth) return;
    const path = notificationTarget.take();
    if (path) setLocation(path);
  }, [pending, unlocked, location, setLocation]);

  return null;
}
