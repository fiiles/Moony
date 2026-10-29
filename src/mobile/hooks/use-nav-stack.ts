import { createContext, useContext, useSyncExternalStore } from 'react';
import { navReduce, type NavEntry, type NavEvent } from '@/mobile/lib/nav-stack';

/**
 * The phone's stack of screens (`nav-stack.ts`) kept in step with the browser history, and
 * whether the swipe back shows the screen under the top one. wouter announces its pushes and
 * replaces as `pushState` / `replaceState` events on `window` (it patches `history` once, before
 * this module runs); back and forward are `popstate`. The stack starts with the page it loads on.
 */

const here = () => `${window.location.pathname}${window.location.search}`;

let stack: readonly NavEntry[] = [{ path: here(), scroll: 0 }];
let underlay = false;
let listeners: (() => void)[] = [];
const emit = () => listeners.forEach((listener) => listener());

const subscribe = (listener: () => void) => {
  listeners = [...listeners, listener];
  return () => {
    listeners = listeners.filter((l) => l !== listener);
  };
};

const onHistory = (type: NavEvent['type']) => () => {
  const next = navReduce(stack, { type, path: here() });
  // Any move ends a swipe's underlay: the screen it showed is now the page itself, or gone
  if (next === stack && !underlay) return;
  stack = next;
  underlay = false;
  emit();
};

if (typeof window !== 'undefined') {
  window.addEventListener('pushState', onHistory('push'));
  window.addEventListener('replaceState', onHistory('replace'));
  window.addEventListener('popstate', onHistory('pop'));
}

/** The stack as it stands; a new array after every change. */
export function useNavStack(): readonly NavEntry[] {
  return useSyncExternalStore(subscribe, () => stack);
}

/** Whether the screen under the top one is drawn (a swipe back is under way). */
export function useNavUnderlay(): boolean {
  return useSyncExternalStore(subscribe, () => underlay);
}

/** Draws (or drops) the screen under the top one, for the swipe back. */
export function setNavUnderlay(shown: boolean): void {
  if (underlay === shown) return;
  underlay = shown;
  emit();
}

/** True when there is a screen under the top one to draw. */
export function hasScreenUnder(): boolean {
  return stack.length > 1;
}

/** Keeps where entry `index` is scrolled to; it is restored when the screen comes back. */
export function saveNavScroll(index: number, scroll: number): void {
  const entry = stack[index];
  // The entry object is updated in place: a scroll is not a change anyone redraws for
  if (entry) (entry as NavEntry).scroll = scroll;
}

/** Where entry `index` was scrolled to when it was left (0 for a new screen). */
export function navScroll(index: number): number {
  return stack[index]?.scroll ?? 0;
}

/**
 * The stack entry a screen stands for: the top for the page, the one under it for the swipe's
 * underlay (`MobileShell`); `null` outside the shell's routes (no scroll is kept).
 */
export const NavEntryContext = createContext<{ index: number; underlay: boolean } | null>(null);

export function useNavEntry() {
  return useContext(NavEntryContext);
}
