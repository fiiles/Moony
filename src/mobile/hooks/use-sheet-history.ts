import { useEffect, useRef } from 'react';
import { createSheetHistory, type SheetHistory } from '@/mobile/lib/sheet-history';

let shared: SheetHistory | undefined;

/** The one stack of sheet entries of the window, created with its `popstate` listener on first use. */
function sheetHistory(): SheetHistory {
  if (!shared) {
    const stack = createSheetHistory(window.history);
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
 * (`createSheetHistory`).
 */
export function useSheetHistory(open: boolean, close: () => void): void {
  const closeRef = useRef(close);
  useEffect(() => {
    closeRef.current = close;
  });

  useEffect(() => {
    if (!open) return;
    return sheetHistory().open(() => closeRef.current());
  }, [open]);
}
