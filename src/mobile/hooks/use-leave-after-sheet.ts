/** How long to wait for the sheet's `popstate` before leaving without it. */
const FALLBACK_MS = 400;

/**
 * Closes a sheet and, once its history entry is off the stack, leaves the screen underneath. An
 * open sheet owns one history entry (`useSheetHistory`) and takes it off again, asynchronously,
 * when it closes — so a screen that left at once (`history.back()` or a `replace`) would land on
 * that entry instead of the one it is meant to go back to, and the screen it is leaving would stay
 * in the history behind it. The `popstate` of the sheet's own pop is the signal that the entry is
 * gone: `leave` runs on it, once. If none arrives (the sheet owned no entry) `leave` runs after
 * `FALLBACK_MS` instead.
 *
 * A plain function, not a hook: it keeps no state, and its caller is a mutation callback that may
 * outlive the component's render.
 */
export function leaveAfterSheet(closeSheet: () => void, leave: () => void): void {
  let left = false;
  const go = () => {
    if (left) return;
    left = true;
    window.removeEventListener('popstate', go);
    clearTimeout(timer);
    leave();
  };
  window.addEventListener('popstate', go);
  const timer = setTimeout(go, FALLBACK_MS);
  closeSheet();
}
