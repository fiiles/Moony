import { useEffect, useLayoutEffect } from 'react';
import { QueryClientProvider } from '@tanstack/react-query';
import { queryClient } from '@/lib/queryClient';
import { Toaster } from '@/components/ui/toaster';
import { TooltipProvider } from '@/components/ui/tooltip';
import { AuthProvider } from '@/hooks/use-auth';
import { CurrencyProvider } from '@/lib/currency-provider';
import { ErrorBoundary } from '@/components/common/ErrorBoundary';
import { I18nProvider } from '@/i18n/I18nProvider';
import { ThemeProvider, useResolvedTheme } from '@/theme/ThemeProvider';
import { SyncProvider } from '@/hooks/SyncProvider';
import { MobileShell } from '@/mobile/shell/MobileShell';
import { useBackgroundLock } from '@/mobile/hooks/use-background-lock';
import { applyStatusBarStyle } from '@/mobile/lib/status-bar';
import { useTypeScale } from '@/mobile/lib/use-type-scale';
import { useVisibleHeight } from '@/mobile/lib/use-visible-height';

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
 * The native status bar follows the resolved theme (`applyStatusBarStyle`): the OS samples the page
 * under the bar and lags behind a live switch, so the style is set outright on every change. Must
 * sit inside `ThemeProvider`; renders nothing.
 */
function StatusBarStyle() {
  const theme = useResolvedTheme();
  useEffect(() => {
    void applyStatusBarStyle(theme);
  }, [theme]);
  return null;
}

/**
 * Root of the phone shell: the provider stack of `App.tsx` without the desktop-only updater,
 * around `MobileShell` (tab bar, nav bar, the four tab roots). The `ThemeProvider` is the
 * desktop's, unchanged: `theme-boot.js` stamps `data-theme` before the first paint, the provider
 * keeps it after that (the OS appearance under `system`) and shows the window once, after its
 * first stamp.
 */
export default function MobileApp() {
  useVisibleHeight();
  useTypeScale();

  // `index.css` drops the desktop's 1080 px minimum width under this attribute, before the first paint.
  useLayoutEffect(() => {
    const root = document.documentElement;
    root.dataset.shell = 'mobile';
    return () => {
      delete root.dataset.shell;
    };
  }, []);

  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <ThemeProvider>
          <I18nProvider>
            <CurrencyProvider>
              <SyncProvider>
                <TooltipProvider delayDuration={200}>
                  <BackgroundLock />
                  <StatusBarStyle />
                  <ErrorBoundary>
                    <MobileShell />
                  </ErrorBoundary>
                  <Toaster
                    position="bottom-center"
                    offset={TOAST_OFFSET}
                    mobileOffset={TOAST_OFFSET}
                  />
                </TooltipProvider>
              </SyncProvider>
            </CurrencyProvider>
          </I18nProvider>
        </ThemeProvider>
      </AuthProvider>
    </QueryClientProvider>
  );
}
