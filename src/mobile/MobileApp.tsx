import { useEffect, useLayoutEffect } from 'react';
import { QueryClientProvider } from '@tanstack/react-query';
import { queryClient } from '@/lib/queryClient';
import { Toaster } from '@/components/ui/toaster';
import { TooltipProvider } from '@/components/ui/tooltip';
import { AuthProvider } from '@/hooks/use-auth';
import { CurrencyProvider } from '@/lib/currency-provider';
import { ErrorBoundary } from '@/components/common/ErrorBoundary';
import { I18nProvider } from '@/i18n/I18nProvider';
import { SyncProvider } from '@/hooks/SyncProvider';
import { MobileShell } from '@/mobile/shell/MobileShell';
import { HideAmountsProvider } from '@/mobile/hooks/use-hide-amounts';
import { useBackgroundLock } from '@/mobile/hooks/use-background-lock';
import { useVisibleHeight } from '@/mobile/lib/use-visible-height';
import { showWindow } from '@/theme/window-theme';

/**
 * Toasts sit above the tab bar (spec §10.3), centred between the 16 pt gutters: 12 pt over the
 * bar and the bottom safe area, so a toast's "Vrátit" is never under the bar. Sonner reads
 * `mobileOffset` below 600 px and `offset` above it (a phone app run in a wide window).
 */
const TOAST_OFFSET = {
  bottom: 'calc(var(--m-tabbar-h) + var(--safe-bottom) + 12px)',
  left: 16,
  right: 16,
};

/**
 * The lock when the app returns from the background or its webview was restored, and the privacy
 * screen. Renders the cover while a lock decision is open: opaque, the canvas colour and nothing
 * else, above everything (sheets sit at z 40–41, the toasts at 999999999), so no unlocked screen
 * shows between the background and the lock page. It also takes the taps meant for what it hides.
 */
function BackgroundLock() {
  const { covered } = useBackgroundLock();
  return covered ? <div aria-hidden className="fixed inset-0 z-[2147483647] bg-canvas" /> : null;
}

/**
 * Root of the phone shell: the provider stack of `App.tsx` without the desktop-only updater,
 * around `MobileShell` (tab bar, nav bar, the four tab roots). The hide-amounts state sits above
 * the shell, so every screen and sheet shares it.
 */
export default function MobileApp() {
  useVisibleHeight();

  // `index.css` drops the desktop's 1080 px minimum width under this attribute, before the first paint.
  useLayoutEffect(() => {
    const root = document.documentElement;
    root.dataset.shell = 'mobile';
    return () => {
      delete root.dataset.shell;
    };
  }, []);

  // The window is configured hidden (`tauri.conf.json`, so the desktop never flashes the wrong theme)
  // and `lib.rs` shows it after 3 s if the frontend never does. The desktop `ThemeProvider` shows it
  // once the theme is stamped; the phone has no provider (light only, `main.tsx`), so it shows the
  // window itself after the first render. tao 0.37 ignores `visible: false` on iOS and Android (the
  // window is up from launch), so this is a no-op there today; the call keeps the phone from relying
  // on that.
  useEffect(() => {
    void showWindow();
  }, []);

  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <I18nProvider>
          <CurrencyProvider>
            <SyncProvider>
              <TooltipProvider delayDuration={200}>
                <HideAmountsProvider>
                  <BackgroundLock />
                  <ErrorBoundary>
                    <MobileShell />
                  </ErrorBoundary>
                </HideAmountsProvider>
                <Toaster
                  position="bottom-center"
                  offset={TOAST_OFFSET}
                  mobileOffset={TOAST_OFFSET}
                />
              </TooltipProvider>
            </SyncProvider>
          </CurrencyProvider>
        </I18nProvider>
      </AuthProvider>
    </QueryClientProvider>
  );
}
