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
                <Toaster />
              </TooltipProvider>
            </SyncProvider>
          </CurrencyProvider>
        </I18nProvider>
      </AuthProvider>
    </QueryClientProvider>
  );
}
