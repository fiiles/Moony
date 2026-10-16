import { useLayoutEffect } from 'react';
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
 * Root of the phone shell: the provider stack of `App.tsx` without the desktop-only updater,
 * around `MobileShell` (tab bar, nav bar, the four tab roots).
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

  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <I18nProvider>
          <CurrencyProvider>
            <SyncProvider>
              <TooltipProvider delayDuration={200}>
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
      </AuthProvider>
    </QueryClientProvider>
  );
}
