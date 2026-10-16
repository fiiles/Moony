import { useEffect, useId, useRef } from 'react';

/** Key of the history state that marks the entry a sheet pushed. */
const MARK = 'moonySheet';

/**
 * Android back closes an open sheet (spec §10.3). While `open`, the sheet owns one history entry
 * (the same URL, a marked state), pushed when it opens: the back button pops it, the `popstate`
 * closes the sheet, and the screen under it stays where it was. When the sheet closes by any
 * other means (the scrim, Zrušit, a save), its entry is taken off again so that back still goes
 * where it would have gone.
 */
export function useSheetHistory(open: boolean, close: () => void): void {
  const token = useId();
  const closeRef = useRef(close);
  useEffect(() => {
    closeRef.current = close;
  });

  useEffect(() => {
    if (!open) return;
    const current: unknown = window.history.state;
    const state = current !== null && typeof current === 'object' ? current : {};
    window.history.pushState({ ...state, [MARK]: token }, '');

    // True once the browser has popped the entry (back), so there is nothing left to take off.
    let popped = false;
    const onPopState = () => {
      popped = true;
      closeRef.current();
    };
    window.addEventListener('popstate', onPopState);

    return () => {
      window.removeEventListener('popstate', onPopState);
      const now: unknown = window.history.state;
      const ours =
        now !== null && typeof now === 'object' && (now as Record<string, unknown>)[MARK] === token;
      if (!popped && ours) window.history.back();
    };
  }, [open, token]);
}
