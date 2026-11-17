import { settingsPath } from '@/mobile/shell/routes';
import { intentState, withIntent } from '@/mobile/lib/row-intent';

/** Where "Připojit znovu" goes, as wouter's `navigate(to, { replace, state })` takes it. */
export interface JoinChoiceNavigation {
  to: string;
  replace: boolean;
  state: Record<string, unknown>;
}

/**
 * "Připojit znovu" on the removed-device sheet: Nastavení → Sync with the join choice open. The
 * choice travels as the entry's intent (`row-intent.ts`), which the page reads and drops before the
 * choice's sheet takes its own entry, so back after cancelling it leaves Sync and never opens it
 * again. From another screen the Sync screen is pushed; on Sync itself the intent goes on the
 * current entry (`replace`, its other keys kept), so no second Sync entry is left for back to land
 * on. Pure: the path and the history state arrive as values.
 */
export function joinChoiceNavigation(path: string, state: unknown): JoinChoiceNavigation {
  const to = settingsPath('sync');
  return path === to
    ? { to, replace: true, state: withIntent(state, 'join') }
    : { to, replace: false, state: intentState(undefined, 'join') };
}
