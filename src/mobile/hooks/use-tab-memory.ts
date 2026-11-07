import { useEffect } from 'react';
import type { NavEntry } from '@/mobile/lib/nav-stack';
import { tabOfEntry, tabTapPlan, tabTrail } from '@/mobile/lib/tab-memory';
import type { TabId } from '@/mobile/shell/routes';
import { navStack, popNavTo, pushNavTrail } from './use-nav-stack';

/**
 * Each tab's place for as long as the app runs (`tab-memory.ts`; audit C3): the screens of the tab
 * from its root to the one last shown on it, with their scroll. In memory only, never written to
 * storage; the entries are the stack's own, so a scroll kept after the snapshot is in them too.
 */
const memory = new Map<TabId, readonly NavEntry[]>();

/** The window event that scrolls the screen shown to its top (a tap on the tab it is the root of). */
export const SCROLL_TOP_EVENT = 'moony:scroll-top';

/** Keeps the place of the tab on top of `stack`, on every change of the stack. */
export function useTabMemory(stack: readonly NavEntry[]): void {
  useEffect(() => {
    const tab = tabOfEntry(stack[stack.length - 1]);
    if (tab) memory.set(tab, tabTrail(stack, tab).entries);
  }, [stack]);
}

/** Forgets every tab's place (no profile any more). */
export function clearTabMemory(): void {
  memory.clear();
}

/**
 * A tap on `tab` in the tab bar: another tab comes back where it was left, the tab shown goes
 * back to its root or, at its root, scrolls to the top (`tabTapPlan`).
 */
export function tapTab(tab: TabId, navigate: (path: string) => void): void {
  const plan = tabTapPlan(navStack(), tab, memory.get(tab));
  switch (plan.kind) {
    case 'scrollTop':
      window.dispatchEvent(new Event(SCROLL_TOP_EVENT));
      return;
    case 'popTo':
      popNavTo(plan.index);
      return;
    case 'push':
      pushNavTrail(plan.entries, navigate);
  }
}
