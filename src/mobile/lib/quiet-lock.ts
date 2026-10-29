/**
 * A lock the person asked for ("Zamknout aplikaci") leaves the lock screen quiet: no Face ID
 * prompt by itself, only the button (owner, 2026-10-29). A lock at start or on the way back from
 * the background still prompts. Settings asks for a quiet lock right before it locks; the lock
 * page takes the request once, when it mounts for that lock.
 */
let quietLockRequested = false;

/** "Zamknout aplikaci" is about to lock. */
export function requestQuietLock(): void {
  quietLockRequested = true;
}

/** The lock failed: the request must not silence a later lock. */
export function clearQuietLock(): void {
  quietLockRequested = false;
}

/** Whether the lock that just landed was a quiet one; answers true once per request. */
export function takeQuietLock(): boolean {
  const quiet = quietLockRequested;
  quietLockRequested = false;
  return quiet;
}
