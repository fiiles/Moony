/** What `whenVisible` needs of the document (a test passes its own). */
export interface VisibilitySource {
  readonly visibilityState: DocumentVisibilityState;
  addEventListener(type: 'visibilitychange', listener: () => void): void;
  removeEventListener(type: 'visibilitychange', listener: () => void): void;
}

/**
 * Runs `callback` once the app is visible: at once when it already is, otherwise on the first
 * `visibilitychange` to visible. Returns the function that cancels the wait (a no-op once the
 * callback has run). The lock screen opens the Face ID prompt this way: a prompt opened while the
 * app is still coming back from the background is dismissed by the transition.
 */
export function whenVisible(callback: () => void, source: VisibilitySource = document): () => void {
  if (source.visibilityState === 'visible') {
    callback();
    return () => {};
  }
  const onChange = () => {
    if (source.visibilityState !== 'visible') return;
    source.removeEventListener('visibilitychange', onChange);
    callback();
  };
  source.addEventListener('visibilitychange', onChange);
  return () => source.removeEventListener('visibilitychange', onChange);
}
