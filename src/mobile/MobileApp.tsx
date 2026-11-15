import { useEffect, useLayoutEffect } from 'react';
import { QueryClientProvider } from '@tanstack/react-query';
import { queryClient } from '@/lib/queryClient';
import { Toaster } from '@/components/ui/toaster';
import { TooltipProvider } from '@/components/ui/tooltip';
import { AuthProvider } from '@/hooks/use-auth';
import { CurrencyProvider } from '@/lib/currency-provider';
import { FormatStyleContext } from '@/lib/use-format';
import { ErrorBoundary } from '@/components/common/ErrorBoundary';
import { I18nProvider, useLanguage } from '@/i18n/I18nProvider';
import { ThemeProvider, useResolvedTheme } from '@/theme/ThemeProvider';
import { SyncProvider } from '@/hooks/SyncProvider';
import { MobileShell } from '@/mobile/shell/MobileShell';
import { useBackgroundLock } from '@/mobile/hooks/use-background-lock';
import { setAmountInputLocale } from '@/mobile/lib/amount-input';
import { applyStatusBarStyle } from '@/mobile/lib/status-bar';
import { useTypeScale } from '@/mobile/lib/use-type-scale';
import { useVisibleHeight } from '@/mobile/lib/use-visible-height';

/**
 * Toasts sit above the tab bar (spec §10.3), centred between the 16 pt gutters: 12 pt over the
 * bar and the bottom safe area, so a toast's "Vrátit" is never under the bar; on a screen without
 * the bar, 12 pt over the safe area (`--m-toast-lift`, `index.css`). Sonner reads `mobileOffset`
 * below 600 px and `offset` above it (a phone app run in a wide window).
 */
const TOAST_OFFSET = {
  bottom: 'calc(var(--m-toast-lift, 0px) + var(--safe-bottom) + 12px)',
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
 * The amount fields read a lone "." or "," the way the UI language groups thousands (audit A7:
 * a German "1.234" is 1234): `parseAmountInput` takes this locale when its caller names none. Set
 * during render, so the first field already reads with it; must sit inside `I18nProvider`.
 */
function AmountInputLocale() {
  const locale = useLanguage().getLocale();
  setAmountInputLocale(locale);
  useEffect(() => () => setAmountInputLocale(undefined), []);
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
      {/* The phone's date table (README §13, "Dates and periods") for every formatter under it */}
      <FormatStyleContext.Provider value="phone">
        <AuthProvider>
          <ThemeProvider>
            <I18nProvider>
              <CurrencyProvider>
                <SyncProvider>
                  <TooltipProvider delayDuration={200}>
                    <BackgroundLock />
                    <StatusBarStyle />
                    <AmountInputLocale />
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
      </FormatStyleContext.Provider>
    </QueryClientProvider>
  );
}
