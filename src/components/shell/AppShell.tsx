import type { ReactNode } from 'react';
import { AppSidebar } from '@/components/shell/AppSidebar';
import { TopBar } from '@/components/shell/TopBar';
import { ShellProvider } from '@/components/shell/ShellProvider';
import { PageTransition } from '@/components/shell/PageTransition';

/**
 * Application frame (design system §4): a 264 px sidebar (E1) beside the
 * page column — max 1480 px, 32 px top, 48 px sides, 56 px bottom — with the
 * top bar (breadcrumb left, data status right) above the page content.
 */
export function AppShell({ children }: { children: ReactNode }) {
  return (
    <ShellProvider>
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
