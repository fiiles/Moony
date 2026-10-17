/**
 * Runs `run` once the history has settled after a sheet closed: on the first `popstate` (the one
 * the sheet's own `history.back()` raises, a tick after it closed) or, when none comes, after
 * `fallbackMs` (the sheet was closed by Android back, so its `popstate` is already behind us).
 * A URL written before that would land on the sheet's marked entry and be popped away with it.
 * Returns the function that cancels the wait (a no-op once `run` has run).
 */
export function afterHistorySettles(
  target: Pick<EventTarget, 'addEventListener' | 'removeEventListener'>,
  run: () => void,
  fallbackMs = 350
): () => void {
  function cancel() {
    target.removeEventListener('popstate', fire);
    clearTimeout(timer);
  }
  function fire() {
    cancel();
    run();
  }
  target.addEventListener('popstate', fire);
  const timer = setTimeout(fire, fallbackMs);
  return cancel;
}
