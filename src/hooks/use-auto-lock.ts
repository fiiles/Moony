/**
 * Auto-lock after inactivity. `useAutoLock()` is mounted once in the
 * authenticated layout (AppLayout in App.tsx); `useAutoLockMinutes()` is the
 * setting the Security page edits. Deadline maths: src/utils/auto-lock.ts.
 */
import { useCallback, useEffect, useRef, useSyncExternalStore } from 'react';
import { useLocation } from 'wouter';
import { useAuth } from '@/hooks/use-auth';
import {
  DEFAULT_AUTO_LOCK_MINUTES,
  isAutoLockDue,
  msUntilAutoLock,
  readAutoLockMinutes,
  shouldRecordActivity,
  writeAutoLockMinutes,
  type AutoLockMinutes,
} from '@/utils/auto-lock';

// Same-window subscribers (the hook below and the settings card) share the
// setting through localStorage; other windows are covered by the `storage` event.
const listeners = new Set<() => void>();

function subscribe(onChange: () => void) {
  listeners.add(onChange);
  window.addEventListener('storage', onChange);
  return () => {
    listeners.delete(onChange);
    window.removeEventListener('storage', onChange);
  };
}

/** The "lock after N minutes" setting and its setter (`0` = off). */
export function useAutoLockMinutes() {
  const minutes = useSyncExternalStore(
    subscribe,
    () => readAutoLockMinutes(),
    () => DEFAULT_AUTO_LOCK_MINUTES
  );

  const setMinutes = useCallback((value: AutoLockMinutes) => {
    writeAutoLockMinutes(value);
    listeners.forEach((listener) => listener());
  }, []);

  return [minutes, setMinutes] as const;
}

/** User input that counts as activity. `wheel` and `touchstart` cover scrolling without clicks. */
const ACTIVITY_EVENTS = ['pointerdown', 'keydown', 'wheel', 'touchstart'] as const;

/**
 * Locks the app (the sidebar's "Lock application" mutation) after the configured
 * minutes without input, and on wake-up when that time has passed while the
 * computer slept or the window was hidden. Inert while locked or when off.
 */
export function useAutoLock(): void {
  const { user, lockMutation } = useAuth();
  const [, setLocation] = useLocation();
  const [minutes] = useAutoLockMinutes();
  const enabled = user !== null && minutes > 0;

  // The latest lock action, so the listeners below are not re-attached on every render.
  // Resolves false when locking failed (useAuth's onError already showed the toast).
  const lockRef = useRef<() => Promise<boolean>>(async () => false);
  useEffect(() => {
    lockRef.current = async () => {
      try {
        await lockMutation.mutateAsync();
        setLocation('/auth', { replace: true });
        return true;
      } catch {
        return false;
      }
    };
  });

  useEffect(() => {
    if (!enabled) return;

    let lastActivity = Date.now();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let locking = false;

    // Wait for the deadline; activity only moves `lastActivity`, so a timer that fires
    // early simply re-arms for the remainder.
    const arm = () => {
      clearTimeout(timer);
      const wait = msUntilAutoLock(Date.now(), lastActivity, minutes);
      if (wait !== null) timer = setTimeout(evaluate, wait);
    };

    const lockNow = async () => {
      if (locking) return;
      locking = true;
      const locked = await lockRef.current();
      locking = false;
      if (!locked) {
        // Retry after a full interval instead of hammering a failing lock.
        lastActivity = Date.now();
        arm();
      }
    };

    const evaluate = () => {
      if (isAutoLockDue(Date.now(), lastActivity, minutes)) void lockNow();
      else arm();
    };

    // The deadline is checked before the activity is recorded: after sleep the first
    // key press or click must lock, not extend a lapsed session.
    const onActivity = () => {
      const now = Date.now();
      if (isAutoLockDue(now, lastActivity, minutes)) {
        void lockNow();
        return;
      }
      if (shouldRecordActivity(now, lastActivity)) lastActivity = now;
    };

    const onWake = () => {
      if (document.visibilityState === 'hidden') return;
      const now = Date.now();
      if (isAutoLockDue(now, lastActivity, minutes)) void lockNow();
      else lastActivity = now;
    };

    const options = { capture: true, passive: true } as const;
    for (const type of ACTIVITY_EVENTS) window.addEventListener(type, onActivity, options);
    document.addEventListener('visibilitychange', onWake);
    window.addEventListener('focus', onWake);
    arm();

    return () => {
      clearTimeout(timer);
      for (const type of ACTIVITY_EVENTS) window.removeEventListener(type, onActivity, options);
      document.removeEventListener('visibilitychange', onWake);
      window.removeEventListener('focus', onWake);
    };
  }, [enabled, minutes]);
}
