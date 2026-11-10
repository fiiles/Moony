import { useEffect } from 'react';
import { onBackButtonPress } from '@tauri-apps/api/app';

/** Set on `<html>` while back is held: the edge swipe (`useEdgeSwipeBack`) does not start. */
const HOLD_BACK_ATTRIBUTE = 'data-hold-back';

/** Whether something holds back navigation now (`useHoldBack`). */
export function backIsHeld(): boolean {
  return document.documentElement.hasAttribute(HOLD_BACK_ATTRIBUTE);
}

/**
 * While `active`, nothing goes back: the swipe from the left edge does not start (`backIsHeld`) and
 * Android's back button and back gesture do nothing — a registered back-button handler takes over
 * the system's default, which goes back in the webview's history. Where there is no such button (iOS,
 * the browser) the registration is refused, and that is fine. For a cover that must stay over the
 * screen until its work is done (`SyncBusyCover`); the cover takes the touches itself.
 */
export function useHoldBack(active: boolean): void {
  useEffect(() => {
    if (!active) return;
    const root = document.documentElement;
    root.setAttribute(HOLD_BACK_ATTRIBUTE, '');
    let released = false;
    let unregister: (() => Promise<void>) | null = null;
    onBackButtonPress(() => undefined)
      .then((listener) => {
        const drop = () => listener.unregister().catch(() => undefined);
        if (released) void drop();
        else unregister = drop;
      })
      .catch(() => undefined);
    return () => {
      released = true;
      root.removeAttribute(HOLD_BACK_ATTRIBUTE);
      void unregister?.();
    };
  }, [active]);
}
