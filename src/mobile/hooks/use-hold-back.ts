import { useEffect, useRef } from 'react';
import { onBackButtonPress } from '@tauri-apps/api/app';
import { holdBack } from '@/mobile/lib/back-hold';

/**
 * While `active`, nothing goes back: the swipe from the left edge does not start (a hold of
 * `back-hold.ts`, counted, so two layers holding at once release only their own) and
 * Android's back button and back gesture run `onBack` (or nothing) instead — a registered
 * back-button handler takes over the system's default, which goes back in the webview's history.
 * Where there is no such button (iOS, the browser) the registration is refused, and that is fine.
 * For a layer that must stay over the screen until its work is done (`SyncBusyCover`) or that ends
 * its own way (`ScanOverlay` cancels the scan); the layer takes the touches itself.
 */
export function useHoldBack(active: boolean, onBack?: () => void): void {
  const onBackRef = useRef(onBack);
  useEffect(() => {
    onBackRef.current = onBack;
  });

  useEffect(() => {
    if (!active) return;
    const release = holdBack();
    let released = false;
    let unregister: (() => Promise<void>) | null = null;
    onBackButtonPress(() => onBackRef.current?.())
      .then((listener) => {
        const drop = () => listener.unregister().catch(() => undefined);
        if (released) void drop();
        else unregister = drop;
      })
      .catch(() => undefined);
    return () => {
      released = true;
      release();
      void unregister?.();
    };
  }, [active]);
}
