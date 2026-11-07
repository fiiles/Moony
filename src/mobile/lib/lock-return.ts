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

/**
 * The walk back after an unlock, while it is under way (`leaveLock`): from its `history.go` until
 * it has landed and its `popstate` has been handled everywhere. Whatever would touch the history
 * meanwhile waits for it — a sheet opening on the first unlocked render (the Face ID offer, a
 * shared statement's import) would otherwise push its entry first, the walk would land one short
 * and its `popstate` would close that sheet.
 */
export interface ReturnWalk {
  active(): boolean;
  begin(): void;
  /** The walk has landed: what waited runs, then the listeners hear it. */
  end(): void;
  /** Runs `run` once the walk has landed (at once when none is under way). */
  whenDone(run: () => void): void;
  subscribe(listener: () => void): () => void;
}

export function createReturnWalk(): ReturnWalk {
  let active = false;
  let waiting: (() => void)[] = [];
  let listeners: (() => void)[] = [];
  return {
    active: () => active,
    begin() {
      if (active) return;
      active = true;
      listeners.forEach((listener) => listener());
    },
    end() {
      if (!active) return;
      active = false;
      const run = waiting;
      waiting = [];
      run.forEach((task) => task());
      listeners.forEach((listener) => listener());
    },
    whenDone(run) {
      if (active) waiting.push(run);
      else run();
    },
    subscribe(listener) {
      listeners = [...listeners, listener];
      return () => {
        listeners = listeners.filter((l) => l !== listener);
      };
    },
  };
}

/** The app's one walk (`leaveLock` drives it; the sheet history and the shell's sheets wait for it). */
export const returnWalk = createReturnWalk();
