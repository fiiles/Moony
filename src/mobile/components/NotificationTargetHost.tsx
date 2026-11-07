import { useEffect, useSyncExternalStore } from 'react';
import { onAction } from '@tauri-apps/plugin-notification';
import { useLocation } from 'wouter';
import { useAuth } from '@/hooks/use-auth';
import { useAnySheetOpen } from '@/mobile/hooks/use-sheet-history';
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
 * unlocked and not on the lock page the host opens it over the screen shown, once no sheet is open
 * (leaving the screen would drop a sheet's input without its "Zahodit změny?"). While locked
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
  // An open sheet keeps what was typed in it: the target waits until the sheet has closed
  const sheetOpen = useAnySheetOpen();

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
    if (pending === null || !unlocked || sheetOpen || location === routes.auth) return;
    const path = notificationTarget.take();
    if (path) setLocation(path);
  }, [pending, unlocked, sheetOpen, location, setLocation]);

  return null;
}
