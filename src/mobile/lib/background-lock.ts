/**
 * Background lock on the phone (spec §8.1): when the app comes back from the background after
 * the chosen time, it is locked again, and a restored webview locks at start. The pure rules and
 * the two small stores behind them; the listener is `useBackgroundLock`
 * (`src/mobile/hooks/use-background-lock.ts`). Times are epoch milliseconds passed in by the
 * caller.
 *
 * This is separate from the desktop's inactivity lock (`src/utils/auto-lock.ts`): a phone is
 * locked by leaving the app, not by not touching it.
 */

/** localStorage: minutes in the background before the lock; the setting itself arrives in Phase 5. */
export const BACKGROUND_LOCK_KEY = 'moony-background-lock-minutes';

/** "Ihned" (0), 1, 5 or 15 minutes. */
export const BACKGROUND_LOCK_OPTIONS = [0, 1, 5, 15] as const;
export type BackgroundLockMinutes = (typeof BACKGROUND_LOCK_OPTIONS)[number];

export const DEFAULT_BACKGROUND_LOCK_MINUTES: BackgroundLockMinutes = 1;

/** sessionStorage: when the app went to the background (it outlives a reload of the webview). */
export const HIDDEN_AT_KEY = 'moony-hidden-at';

const MS_PER_MINUTE = 60_000;

/**
 * Whether coming back to the foreground locks the app. `hiddenAt` is when it went to the
 * background, `null` when it did not (no lock). `0` minutes locks on every return; otherwise the
 * time away must have reached the threshold (inclusive). A clock that went backwards never
 * reaches it.
 */
export function shouldLockOnResume(
  hiddenAt: number | null,
  now: number,
  afterMinutes: BackgroundLockMinutes
): boolean {
  if (hiddenAt === null) return false;
  if (afterMinutes === 0) return true;
  return now - hiddenAt >= afterMinutes * MS_PER_MINUTE;
}

/** `useAuth().appStatus`. */
export type AuthStatus = 'needs_setup' | 'locked' | 'unlocked';

/**
 * The lock at start (ruling R26): a page that finds the backend already unlocked on its very first
 * status answer did not unlock it itself. Outside a dev reload that is iOS reclaiming the web
 * content process and Tauri reloading the webview, so the app locks at once.
 * - `checking`: the provider is still asking (its `locked` default while `isLoading`);
 * - `due`: the first answer was `unlocked`; the lock is owed until the app is locked;
 * - `done`: nothing owed, for the rest of this page session (a later unlock is the person's own).
 */
export type BootLock = 'checking' | 'due' | 'done';

export function nextBootLock(current: BootLock, status: AuthStatus, isLoading: boolean): BootLock {
  switch (current) {
    case 'checking':
      if (status === 'locked' && isLoading) return 'checking';
      return status === 'unlocked' ? 'due' : 'done';
    case 'due':
      return status === 'unlocked' ? 'due' : 'done';
    case 'done':
      return 'done';
  }
}

/**
 * The opaque cover over the shell: from going to the background until the lock decision on the
 * way back (`hidden`), and while the boot lock is owed. Only over unlocked content: once the app
 * is locked the lock page replaces it.
 */
export function showsLockCover(state: {
  unlocked: boolean;
  hidden: boolean;
  bootLock: BootLock;
}): boolean {
  return state.unlocked && (state.hidden || state.bootLock === 'due');
}

/** The stored setting; anything missing, unreadable or outside the four choices is one minute. */
export function readBackgroundLockMinutes(
  storage: Pick<Storage, 'getItem'> | null | undefined
): BackgroundLockMinutes {
  let raw: string | null = null;
  try {
    raw = storage?.getItem(BACKGROUND_LOCK_KEY) ?? null;
  } catch {
    // Blocked storage: the default.
  }
  if (raw === null || raw.trim() === '') return DEFAULT_BACKGROUND_LOCK_MINUTES;
  const value = Number(raw);
  return (
    BACKGROUND_LOCK_OPTIONS.find((option) => option === value) ?? DEFAULT_BACKGROUND_LOCK_MINUTES
  );
}

/** The stamp left when the app went to the background, `null` when there is none or it is unreadable. */
export function readHiddenAt(storage: Pick<Storage, 'getItem'> | null | undefined): number | null {
  try {
    const raw = storage?.getItem(HIDDEN_AT_KEY);
    if (raw === null || raw === undefined || raw.trim() === '') return null;
    const value = Number(raw);
    return Number.isFinite(value) && value >= 0 ? value : null;
  } catch {
    return null;
  }
}

export function rememberHiddenAt(storage: Pick<Storage, 'setItem'> | null | undefined, at: number) {
  try {
    storage?.setItem(HIDDEN_AT_KEY, String(at));
  } catch {
    // Storage that cannot be written: the in-memory stamp still locks this session's return.
  }
}

export function clearHiddenAt(storage: Pick<Storage, 'removeItem'> | null | undefined) {
  try {
    storage?.removeItem(HIDDEN_AT_KEY);
  } catch {
    // Storage that cannot be written cannot have kept the stamp either.
  }
}
