import type { SyncStatus } from '@shared/schema';

/** What Nastavení's Sync row says: a `deviceSync` key and how it reads (`stale` asks for a look). */
export type SyncRowSummary = { key: string; tone: 'fresh' | 'stale' | 'neutral' };

/**
 * The sub-line of the Sync row in Nastavení (a `deviceSync` key). Off or half set up reads "off"
 * (the page offers the setup); a pause because the phone has no connection is its own line, any
 * other pause or an error asks for a look on the page.
 */
export function syncRowSummary(status: SyncStatus | undefined): SyncRowSummary {
  switch (status?.phase) {
    case 'idle':
      return { key: 'phone.row.synced', tone: 'fresh' };
    case 'syncing':
      return { key: 'phone.row.syncing', tone: 'neutral' };
    case 'paused':
      return status.problem === 'offline'
        ? { key: 'phone.row.offline', tone: 'stale' }
        : { key: 'phone.row.paused', tone: 'stale' };
    case 'error':
      return { key: 'phone.row.error', tone: 'stale' };
    default:
      return { key: 'phone.row.off', tone: 'neutral' };
  }
}
