import { useEffect, useRef, useSyncExternalStore } from 'react';
import { returnWalk } from '@/mobile/lib/lock-return';
import { createSheetHistory, type SheetClose, type SheetHistory } from '@/mobile/lib/sheet-history';

let shared: SheetHistory | undefined;
let listeners: (() => void)[] = [];

/**
 * The one stack of sheet entries of the window, created with its `popstate` listener on first use.
 * A sheet that opens while the walk back after an unlock runs pushes its entry once it has landed.
 */
function sheetHistory(): SheetHistory {
  if (!shared) {
    const stack = createSheetHistory(window.history, {
      walk: returnWalk,
      onChange: () => listeners.forEach((listener) => listener()),
    });
    window.addEventListener('popstate', () => stack.popstate());
    shared = stack;
  }
  return shared;
}

/**
 * Android back closes an open sheet (spec §10.3). While `open`, the sheet owns one history entry
 * (the same URL, a marked state), pushed when it opens: the back button pops it, the `popstate`
 * closes the sheet, and the screen under it stays where it was. Sheets stack — the category
 * picker opens over the transaction sheet — so back closes the newest one only. When a sheet
 * closes by any other means (the scrim, Zrušit, a save), its entry is taken off again so that
 * back still goes where it would have gone, and the sheets beneath it stay open
 * (`createSheetHistory`). `close` may refuse with `false` (a sheet with unsaved input asks first):
 * the sheet then keeps an entry.
 */
export function useSheetHistory(open: boolean, close: SheetClose): void {
  const closeRef = useRef(close);
  useEffect(() => {
    closeRef.current = close;
  });

  useEffect(() => {
    if (!open) return;
    return sheetHistory().open(() => closeRef.current());
  }, [open]);
}

/** How many history entries the sheets hold now (the lock walks back over them, `lock-return.ts`). */
export function sheetEntryCount(): number {
  return shared?.size() ?? 0;
}

/** Whether any sheet holds a history entry now; re-renders when that changes. */
export function useAnySheetOpen(): boolean {
  return useSyncExternalStore(
    (listener) => {
      listeners = [...listeners, listener];
      return () => {
        listeners = listeners.filter((l) => l !== listener);
      };
    },
    () => sheetEntryCount() > 0
  );
}

/**
 * Whether the walk back after an unlock is under way (`leaveLock`): a sheet that would open by
 * itself on the first unlocked render waits for it to land.
 */
export function useReturnWalkActive(): boolean {
  return useSyncExternalStore(returnWalk.subscribe, returnWalk.active);
}
