import { useEffect, useRef } from 'react';
import { useAuth } from '@/hooks/use-auth';
import { keychainApi } from '@/lib/tauri-api';
import { deviceStorage } from '@/utils/device-storage';
import {
  clearHiddenAt,
  readBackgroundLockMinutes,
  readHiddenAt,
  rememberHiddenAt,
  shouldLockOnResume,
} from '@/mobile/lib/background-lock';

/** `sessionStorage` when it is usable: the stamp must outlive a reload of the webview, not the app. */
function sessionStore(): Storage | null {
  try {
    return typeof sessionStorage === 'undefined' ? null : sessionStorage;
  } catch {
    return null;
  }
}

/**
 * Background lock and privacy screen of the phone (spec §8.1), mounted once in the shell.
 *
 * - The privacy screen (the blurred app-switcher snapshot) is asked for once on mount. The plugin
 *   has it on by default; a desktop shell or a plugin that does not know the command rejects, and
 *   that is fine.
 * - While unlocked, going to the background stamps the time (in memory and in `sessionStorage`),
 *   and coming back locks the app through the same `lockMutation` as "Zamknout aplikaci" when
 *   `shouldLockOnResume` says the stay was long enough (the setting
 *   `moony-background-lock-minutes`, default 1 minute). A stamp already there when the app starts
 *   or unlocks means the webview was restored after it left the foreground: lock.
 * - While locked it does nothing, so the Face ID prompt of the lock screen (which deactivates the
 *   app) can never feed another lock.
 *
 * The lock screen prompts for Face ID itself once it is visible (`Lock.tsx`).
 */
export function useBackgroundLock(): void {
  const { appStatus, lockMutation } = useAuth();
  const { mutate: lock } = lockMutation;
  const unlocked = appStatus === 'unlocked';
  const hiddenAt = useRef<number | null>(null);

  useEffect(() => {
    keychainApi.setPrivacyScreen(true).catch(() => {
      // Unsupported (desktop shell) or refused: nothing to do, the app works without it.
    });
  }, []);

  useEffect(() => {
    if (!unlocked) return;
    const session = sessionStore();

    if (readHiddenAt(session) !== null) {
      clearHiddenAt(session);
      lock();
    }

    const onVisibilityChange = () => {
      if (document.visibilityState === 'hidden') {
        // The first stamp stands: a second "hidden" without a return does not shorten the stay.
        if (hiddenAt.current === null) {
          hiddenAt.current = Date.now();
          rememberHiddenAt(session, hiddenAt.current);
        }
        return;
      }
      const since = hiddenAt.current ?? readHiddenAt(session);
      hiddenAt.current = null;
      clearHiddenAt(session);
      if (shouldLockOnResume(since, Date.now(), readBackgroundLockMinutes(deviceStorage()))) {
        lock();
      }
    };

    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => document.removeEventListener('visibilitychange', onVisibilityChange);
  }, [unlocked, lock]);
}
