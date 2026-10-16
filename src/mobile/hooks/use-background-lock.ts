import { useEffect, useRef, useState } from 'react';
import { flushSync } from 'react-dom';
import { useAuth } from '@/hooks/use-auth';
import { keychainApi } from '@/lib/tauri-api';
import { deviceStorage } from '@/utils/device-storage';
import {
  clearHiddenAt,
  nextBootLock,
  readBackgroundLockMinutes,
  readHiddenAt,
  rememberHiddenAt,
  shouldLockOnResume,
  showsLockCover,
  type BootLock,
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
 * Background lock and privacy screen of the phone (spec §8.1, ruling R26), mounted once in the
 * shell. Returns whether the cover is up; `MobileApp` draws it.
 *
 * - The privacy screen (the blurred app-switcher snapshot) is asked for once on mount. The plugin
 *   has it on by default; a desktop shell or a plugin that does not know the command rejects, and
 *   that is fine.
 * - Lock at start: when the provider's very first status answer is already `unlocked`, nothing in
 *   this page unlocked it. Outside a dev reload that is iOS reclaiming the web content process and
 *   Tauri reloading the webview, so the app locks at once (`nextBootLock`), through the same
 *   `lockMutation` as "Zamknout aplikaci".
 * - While unlocked, going to the background stamps the time (in memory and in `sessionStorage`),
 *   and coming back locks the app when `shouldLockOnResume` says the stay was long enough (the
 *   setting `moony-background-lock-minutes`, default 1 minute).
 * - The cover: an opaque screen over everything, put up synchronously when the app goes to the
 *   background and kept on the way back until the lock decision (until the lock lands when one is
 *   due), and while the lock at start is owed. The native blur lifts before the asynchronous lock
 *   lands, so without it the unlocked screen would show for a moment. A lock that fails lifts it
 *   (the provider toasts the failure).
 * - While locked it does nothing, so the Face ID prompt of the lock screen (which deactivates the
 *   app) can never feed another lock.
 *
 * The lock screen prompts for Face ID itself once it is visible (`Lock.tsx`).
 */
export function useBackgroundLock(): { covered: boolean } {
  const { appStatus, isLoading, lockMutation } = useAuth();
  const { mutate: lock } = lockMutation;
  const unlocked = appStatus === 'unlocked';
  const hiddenAt = useRef<number | null>(null);
  // Away: from "hidden" until the decision on the way back.
  const [away, setAway] = useState(false);
  const [bootLock, setBootLock] = useState<BootLock>('checking');
  // The lock at start is sent once per page, whatever re-renders or re-runs the effect.
  const bootLockSent = useRef(false);

  // Bookkeeping during render, so the cover never lags a frame behind the status.
  const nextBoot = nextBootLock(bootLock, appStatus, isLoading);
  if (nextBoot !== bootLock) setBootLock(nextBoot);
  // Locked: the lock page replaces the cover, and the next unlock starts without one.
  if (away && !unlocked) setAway(false);

  useEffect(() => {
    keychainApi.setPrivacyScreen(true).catch(() => {
      // Unsupported (desktop shell) or refused: nothing to do, the app works without it.
    });
  }, []);

  useEffect(() => {
    if (bootLock !== 'due' || bootLockSent.current) return;
    bootLockSent.current = true;
    lock(undefined, {
      // Still unlocked: the screen it has rather than a blank one.
      onError: () => setBootLock('done'),
    });
  }, [bootLock, lock]);

  useEffect(() => {
    if (!unlocked) return;
    const session = sessionStore();
    // A stamp left by the page this one replaced: the lock at start already covers a restored
    // webview, and after a locked start the stamp is stale.
    clearHiddenAt(session);

    const onVisibilityChange = () => {
      if (document.visibilityState === 'hidden') {
        // Committed now, not on the next frame: the last frame before the background is the cover.
        flushSync(() => setAway(true));
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
        // The cover stays until the lock lands and the lock page replaces it.
        lock(undefined, { onError: () => setAway(false) });
        return;
      }
      setAway(false);
    };

    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => {
      document.removeEventListener('visibilitychange', onVisibilityChange);
      // Locked by something else (Settings, an invalidated biometry) or gone: a stamp left from a
      // stay in the background must not lock once more after the next unlock.
      hiddenAt.current = null;
      clearHiddenAt(session);
    };
  }, [unlocked, lock]);

  return { covered: showsLockCover({ unlocked, hidden: away, bootLock }) };
}
