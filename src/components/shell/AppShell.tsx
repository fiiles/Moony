import type { ReactNode } from 'react';
import { AppSidebar } from '@/components/shell/AppSidebar';
import { TopBar } from '@/components/shell/TopBar';
import { ShellProvider } from '@/components/shell/ShellProvider';
import { useDeviceSyncEvents } from '@/hooks/use-device-sync-events';
import { RemovedNoticeDialog } from '@/components/settings/sync/RemovedNotice';
import { SyncBusyModal } from '@/components/settings/sync/SyncBusyModal';
import { PageTransition } from '@/components/shell/PageTransition';
import { useAuth } from '@/hooks/use-auth';

/**
 * Application frame (design system §4): a 264 px sidebar (E1) beside the
 * page column — max 1480 px, 32 px top, 48 px sides, 56 px bottom — with the
 * top bar (breadcrumb left, data status right) above the page content.
 */
export function AppShell({ children }: { children: ReactNode }) {
  useDeviceSyncEvents();
  // The demo profile never syncs (ADR 0037): no sync dialogs that could offer to join
  const { isDemo } = useAuth();
  return (
    <ShellProvider>
      {!isDemo && <SyncBusyModal />}
      {!isDemo && <RemovedNoticeDialog />}
      <div className="grid min-h-screen grid-cols-[var(--sidebar-w)_minmax(0,1fr)]">
        <AppSidebar />
        <main className="min-w-0">
          <div className="mx-auto w-full max-w-page px-[var(--page-pad-x)] pb-14 pt-[var(--page-pad-t)]">
            <TopBar />
            <PageTransition>{children}</PageTransition>
          </div>
        </main>
      </div>
    </ShellProvider>
  );
}
