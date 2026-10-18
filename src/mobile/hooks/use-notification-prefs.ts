import { useCallback, useEffect, useState } from 'react';
import { deviceStorage } from '@/utils/device-storage';
import {
  readNotificationPrefs,
  writeNotificationPrefs,
  type NotificationPrefs,
} from '@/mobile/lib/notification-prefs';

/** Fired on `window` with the new preferences whenever any instance of the hook changes them. */
export const NOTIFICATION_PREFS_EVENT = 'moony-notification-prefs';

/**
 * The notification preferences (`moony-notifications` in `localStorage`) and the way to change
 * them. Every instance follows every change: the setter writes, then tells the others through a
 * `window` event, so the scheduler mounted in the shell plans again when a switch on
 * Nastavení → Oznámení is turned. The event carries the new value, so it does not depend on the
 * storage being readable.
 */
export function useNotificationPrefs(): [NotificationPrefs, (next: NotificationPrefs) => void] {
  const [prefs, setPrefs] = useState(() => readNotificationPrefs(deviceStorage()));

  useEffect(() => {
    const onChange = (event: Event) => setPrefs((event as CustomEvent<NotificationPrefs>).detail);
    window.addEventListener(NOTIFICATION_PREFS_EVENT, onChange);
    return () => window.removeEventListener(NOTIFICATION_PREFS_EVENT, onChange);
  }, []);

  const update = useCallback((next: NotificationPrefs) => {
    writeNotificationPrefs(deviceStorage(), next);
    window.dispatchEvent(new CustomEvent(NOTIFICATION_PREFS_EVENT, { detail: next }));
  }, []);

  return [prefs, update];
}
