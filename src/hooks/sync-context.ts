import { createContext, useContext } from 'react';
import type { BackfillResult } from '@/lib/tauri-api';

export interface SyncContextType {
  isSyncing: boolean;
  progress: { current: number; total: number };
  lastResult: BackfillResult | null;
  startBackfill: () => Promise<void>;
  recordTodaySnapshot: () => Promise<void>;
}

export const SyncContext = createContext<SyncContextType | undefined>(undefined);

export function useSyncStatus() {
  const context = useContext(SyncContext);
  if (!context) {
    throw new Error('useSyncStatus must be used within a SyncProvider');
  }
  return context;
}
