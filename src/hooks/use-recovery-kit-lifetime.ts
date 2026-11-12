import { useEffect, useRef } from 'react';
import { kitExpired, KIT_VIEW_MS } from '@/utils/recovery-kit-lifetime';

/**
 * While a Recovery Kit is on screen: calls `onExpired` once after KIT_VIEW_MS or as soon as the
 * app is hidden, and (desktop only, `protectScreen`) keeps the window out of screenshots and screen
 * recordings. Mount it with the kit; unmounting stops the timer and lifts the protection.
 */
export function useRecoveryKitLifetime(onExpired: () => void, protectScreen: boolean) {
  const expired = useRef(onExpired);
  useEffect(() => {
    expired.current = onExpired;
  });

  useEffect(() => {
    const shownAt = Date.now();
    let done = false;
    const check = () => {
      if (done) return;
      if (kitExpired(shownAt, Date.now(), document.visibilityState === 'hidden')) {
        done = true;
        expired.current();
      }
    };
    const timer = window.setTimeout(check, KIT_VIEW_MS);
    document.addEventListener('visibilitychange', check);
    return () => {
      done = true;
      window.clearTimeout(timer);
      document.removeEventListener('visibilitychange', check);
    };
  }, []);

  useEffect(() => {
    if (!protectScreen) return;
    let cancelled = false;
    const setProtected = async (on: boolean) => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        await getCurrentWindow().setContentProtected(on);
      } catch {
        // Not in Tauri (browser, tests) or refused: the kit still leaves the screen on time.
      }
    };
    void setProtected(true).then(() => {
      if (cancelled) void setProtected(false);
    });
    return () => {
      cancelled = true;
      void setProtected(false);
    };
  }, [protectScreen]);
}
