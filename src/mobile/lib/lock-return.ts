/**
 * Where the phone goes back to after an unlock (spec §8.1, audit C3): the screen the lock covered,
 * scrolled where it was. When the app locks on a screen of the shell, the protected route pushes
 * the lock (`/auth`) over it, and the sheets open on it unmount with the screen while their history
 * entries stay (`sheet-history.ts`). The record says how far back that screen is: the lock's entry
 * and the sheets' entries. The unlock walks back that far, so the history and the screen stack are
 * as they were. Nothing here opens anything: only the unlock reads the record.
 */
import { isShellPath } from '@/mobile/shell/routes';

export interface LockReturn {
  /** The screen the lock covered (with its search). */
  path: string;
  /** History entries between it and the lock page: the lock's own and those of the open sheets. */
  depth: number;
}

export interface LockReturnStore {
  /**
   * The app has just locked while `path` was shown, with `sheetEntries` sheet entries open on it.
   * A path outside the shell (the lock page itself) leaves nothing to return to.
   */
  locked(path: string, sheetEntries: number): void;
  /** The record, once: the unlock that reads it is the only one that returns. */
  take(): LockReturn | null;
  /** Forgets the record (no profile any more, or a new one). */
  clear(): void;
}

export function createLockReturn(): LockReturnStore {
  let record: LockReturn | null = null;
  return {
    locked(path, sheetEntries) {
      record = isShellPath(path) ? { path, depth: 1 + Math.max(0, sheetEntries) } : null;
    },
    take() {
      const taken = record;
      record = null;
      return taken;
    },
    clear() {
      record = null;
    },
  };
}

/** The app's one record (`MobileShell` writes it, `Lock` reads it). */
export const lockReturn = createLockReturn();
