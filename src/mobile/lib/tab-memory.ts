/**
 * What each tab remembers of where it was left (spec §6, audit C3), and what a tap on the tab bar
 * does with it. Pure: the store is `MobileShell`'s, the taps are the `TabBar`'s. A tab's place is
 * its trail: the screens of that tab on top of the stack, from its root to the screen shown, each
 * with its scroll. A tap on another tab pushes that trail again, so the screen comes back and back
 * still walks the tab's own screens (Finance under an account, as on iOS); a tap on the tab shown
 * goes back to its root, or scrolls the root to its top.
 */
import type { NavEntry } from '@/mobile/lib/nav-stack';
import { routes, tabForPath, type TabId } from '@/mobile/shell/routes';

/** A path without its search (`/cashflow?o=-1` → `/cashflow`). */
export function pathOnly(path: string): string {
  const cut = path.search(/[?#]/);
  return cut === -1 ? path : path.slice(0, cut);
}

/** The tab a stack entry belongs to (`null` outside the tabs: the lock, Nastavení). */
export function tabOfEntry(entry: NavEntry | undefined): TabId | null {
  return entry ? tabForPath(pathOnly(entry.path)) : null;
}

const isRoot = (entry: NavEntry, tab: TabId) => pathOnly(entry.path) === routes[tab];

/**
 * The screens of `tab` on top of the stack, oldest first, from the tab's last root among them
 * (when they hold one), and where they start in the stack. Empty when the top is not on `tab`.
 */
export function tabTrail(
  stack: readonly NavEntry[],
  tab: TabId
): { entries: NavEntry[]; start: number } {
  let start = stack.length;
  while (start > 0 && tabOfEntry(stack[start - 1]) === tab) start--;
  for (let i = stack.length - 1; i >= start; i--) {
    if (isRoot(stack[i], tab)) {
      start = i;
      break;
    }
  }
  return { entries: stack.slice(start), start };
}

export type TabTap =
  /** The tab's root is shown: scroll it to its top. */
  | { kind: 'scrollTop' }
  /** Go back to the tab's root, at `index` in the stack, in one step. */
  | { kind: 'popTo'; index: number }
  /** Push these screens one after another (the tab's remembered place, its root first). */
  | { kind: 'push'; entries: NavEntry[] };

/**
 * What a tap on `tab` does. On the tab shown: its root scrolls up; deeper, back to the root when it
 * is below in the stack, else the root is pushed (the screen was opened from another tab). On
 * another tab: its remembered trail is pushed, its root first (added when the trail was entered
 * from another tab), or just the root when nothing is remembered.
 */
export function tabTapPlan(
  stack: readonly NavEntry[],
  tab: TabId,
  remembered: readonly NavEntry[] | undefined
): TabTap {
  const root: NavEntry = { path: routes[tab], scroll: 0 };
  const top = stack[stack.length - 1];
  if (tabOfEntry(top) === tab) {
    if (isRoot(top, tab)) return { kind: 'scrollTop' };
    const { entries, start } = tabTrail(stack, tab);
    return entries.length > 0 && isRoot(entries[0], tab)
      ? { kind: 'popTo', index: start }
      : { kind: 'push', entries: [root] };
  }
  const trail = (remembered ?? []).filter((entry) => tabOfEntry(entry) === tab);
  if (trail.length === 0) return { kind: 'push', entries: [root] };
  return { kind: 'push', entries: isRoot(trail[0], tab) ? [...trail] : [root, ...trail] };
}
