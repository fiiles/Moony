import type { SyncStatus } from '@shared/schema';

export type SyncIndicator = {
  tone: 'fresh' | 'stale' | 'neutral';
  /** Key in the `deviceSync` namespace. */
  labelKey: string;
} | null;

/** The top-bar indicator for a sync status; `null` while sync is off or being set up. */
export function syncIndicator(status: SyncStatus | undefined): SyncIndicator {
  if (!status || status.phase === 'off' || status.phase === 'settingUp') return null;
  if (status.problem === 'offline') return { tone: 'stale', labelKey: 'indicator.offline' };
  switch (status.phase) {
    case 'idle':
      return { tone: 'fresh', labelKey: 'indicator.synced' };
    case 'syncing':
      return { tone: 'neutral', labelKey: 'indicator.syncing' };
    case 'waiting':
      return { tone: 'stale', labelKey: 'indicator.waiting' };
    case 'paused':
      return { tone: 'stale', labelKey: 'indicator.paused' };
    case 'error':
      return { tone: 'stale', labelKey: 'indicator.error' };
  }
}
