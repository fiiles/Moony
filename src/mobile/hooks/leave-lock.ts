import { lockReturn } from '@/mobile/lib/lock-return';
import { notificationTarget } from '@/mobile/lib/notification-target';
import { routes } from '@/mobile/shell/routes';

/** How long to wait for the walk back before the lock page is replaced instead. */
const FALLBACK_MS = 2000;

type Navigate = (path: string, options?: { replace?: boolean }) => void;

/**
 * Leaves the lock page after a successful unlock (spec §8.1; audit C3, C4). Called only by the
 * lock page once it is unlocked, so nothing here can open a screen behind the lock.
 *
 * - The screen the lock covered (`lockReturn`) comes back: the history walks back over the lock's
 *   entry and the entries of the sheets that were open, so the screen stack, the back label and
 *   the scroll are as they were. Should the walk not arrive (no `popstate` within `FALLBACK_MS`)
 *   or land on the lock page again, the lock page is replaced with that screen instead.
 * - A tapped notification's target (`notificationTarget`) then opens over it, so back from the
 *   target goes to the screen before the lock.
 * - Without either (a cold start), Přehled replaces the lock page: back from it does not return to
 *   the lock.
 */
export function leaveLock(navigate: Navigate): void {
  const target = notificationTarget.take();
  const back = lockReturn.take();
  // The walk back is only possible while the history holds that many entries behind the lock
  // (a lock pushes its page, which drops any forward entries, so the current one is the last)
  if (!back || back.depth >= window.history.length) {
    navigate(target ?? back?.path ?? routes.overview, { replace: true });
    return;
  }
  let done = false;
  const arrive = () => {
    if (done) return;
    done = true;
    window.removeEventListener('popstate', arrive);
    window.clearTimeout(timer);
    if (window.location.pathname === routes.auth) navigate(back.path, { replace: true });
    if (target) navigate(target);
  };
  window.addEventListener('popstate', arrive);
  const timer = window.setTimeout(arrive, FALLBACK_MS);
  window.history.go(-back.depth);
}
