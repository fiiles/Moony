import { afterHistorySettles } from '@/mobile/lib/history-settle';
import { menusHoldHistory, type ShellEnv } from '@/lib/platform';

/**
 * Goes to `to` from a menu item. Where an open menu owns a history entry (Android,
 * `menusHoldHistory`), the route waits until the closing menu has popped its entry: written at
 * once it would land above the menu's marked entry, which then stays behind the new screen and
 * eats the next back press. Elsewhere it goes at once.
 */
export function navigateFromMenu(
  navigate: (to: string) => void,
  to: string,
  env: ShellEnv = import.meta.env,
  target: Pick<EventTarget, 'addEventListener' | 'removeEventListener'> = window
): void {
  if (!menusHoldHistory(env)) {
    navigate(to);
    return;
  }
  afterHistorySettles(target, () => navigate(to));
}
