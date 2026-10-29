/**
 * The phone's own record of the screens it has pushed, as the browser history holds them but
 * readable: the swipe back draws the screen under the top one (`MobileShell`) and every screen
 * comes back scrolled where it was left (`Screen`). Pure; the store that feeds it the history's
 * pushes, replaces and pops is `nav-store.ts`. A path carries its search (`/cashflow?o=-1`).
 */

export interface NavEntry {
  path: string;
  /** The screen's scroll offset when it was left (px). */
  scroll: number;
}

export interface NavEvent {
  type: 'push' | 'replace' | 'pop';
  /** Where the history stands after the change. */
  path: string;
}

/**
 * The stack after a history change: a push adds a screen at its start, unless it is the top's own
 * path again (an open sheet's history entry, `sheet-history.ts`); a replace renames the top
 * and keeps its scroll; a pop to the screen under the top drops the top, a pop that stays on the
 * top (a sheet's history entry going) changes nothing, and a pop anywhere else (forward, or into
 * history from before the app knew it) is taken as a new screen.
 */
export function navReduce(stack: readonly NavEntry[], event: NavEvent): readonly NavEntry[] {
  const top = stack[stack.length - 1];
  switch (event.type) {
    case 'push':
      return top?.path === event.path ? stack : [...stack, { path: event.path, scroll: 0 }];
    case 'replace':
      return top ? [...stack.slice(0, -1), { ...top, path: event.path }] : stack;
    case 'pop': {
      if (top?.path === event.path) return stack;
      const under = stack[stack.length - 2];
      if (under?.path === event.path) return stack.slice(0, -1);
      return [...stack, { path: event.path, scroll: 0 }];
    }
  }
}
