import { createContext, useContext, useEffect } from 'react';

/**
 * What a page tells the shell about itself (design system §5): the entity
 * name that ends the breadcrumb on a detail page, and an optional status text
 * for the top-bar right ("Výpočet probíhá lokálně, nic se neukládá"). When a
 * page sets nothing, the top bar shows the price-data status.
 */
export type ShellStatusTone = 'neutral' | 'fresh' | 'stale';

export interface ShellStatus {
  text: string;
  tone?: ShellStatusTone;
}

export interface ShellPageInfo {
  /** Last breadcrumb segment on detail and sub-pages (entity name, settings section). */
  crumb?: string;
  status?: ShellStatus;
}

export interface ShellContextValue {
  page: ShellPageInfo;
  setPage: (info: ShellPageInfo) => void;
}

/** Provided by ShellProvider in ./ShellProvider.tsx; use useShell() elsewhere. */
export const ShellContext = createContext<ShellContextValue | null>(null);

export function useShell(): ShellContextValue {
  const ctx = useContext(ShellContext);
  if (!ctx) throw new Error('useShell must be used within ShellProvider');
  return ctx;
}

/**
 * Publishes the page's breadcrumb tail and status while the page is mounted.
 * Pass plain strings; the page translates them. Changing values update the
 * top bar, unmounting clears it.
 */
export function useShellPage(info: ShellPageInfo): void {
  const { setPage } = useShell();
  const { crumb } = info;
  const statusText = info.status?.text;
  const statusTone = info.status?.tone;
  useEffect(() => {
    setPage({
      crumb,
      status: statusText ? { text: statusText, tone: statusTone } : undefined,
    });
    return () => setPage({});
  }, [setPage, crumb, statusText, statusTone]);
}
