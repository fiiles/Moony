import { QueryClientProvider } from '@tanstack/react-query';
import { queryClient } from '@/lib/queryClient';
import { Toaster } from '@/components/ui/toaster';
import { TooltipProvider } from '@/components/ui/tooltip';
import { AuthProvider } from '@/hooks/use-auth';
import { CurrencyProvider } from '@/lib/currency-provider';
import { ErrorBoundary } from '@/components/common/ErrorBoundary';
import { I18nProvider } from '@/i18n/I18nProvider';
import { SyncProvider } from '@/hooks/SyncProvider';

/**
 * Root of the phone shell: the provider stack of `App.tsx` without the desktop-only updater.
 * The placeholder is replaced by `MobileShell` (tab bar, lock, overview) in Task 1.3.
 */
export default function MobileApp() {
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <I18nProvider>
          <CurrencyProvider>
            <SyncProvider>
              <TooltipProvider delayDuration={200}>
                <ErrorBoundary>
                  {/* Pinned to the viewport: the desktop CSS gives `body` a 1080 px minimum width. */}
                  <main className="fixed inset-0 flex items-center justify-center text-h3 text-ink">
                    Moony
                  </main>
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
