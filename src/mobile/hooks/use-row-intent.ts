import { useCallback, useEffect, useRef } from 'react';
import { useLocation } from 'wouter';
import { backState } from '@/mobile/lib/back-label';
import { intentState, readIntent, withoutIntent, type RowIntent } from '@/mobile/lib/row-intent';

/**
 * A main row's menu item (README §13, Gestures): opens `path` with `intent` already open there.
 * `from` is the label of the screen the row is on when the push leaves its tab (`backState`).
 */
export function useOpenWithIntent(): (path: string, intent: RowIntent, from?: string) => void {
  const [, setLocation] = useLocation();
  return useCallback(
    (path, intent, from) => setLocation(path, { state: intentState(backState(from), intent) }),
    [setLocation]
  );
}

/**
 * The detail's side of a row's intent: once `ready` (what the act needs has loaded), reads the
 * intent the push left in the history entry, drops it from the entry (back and forward never open
 * the act again) and runs its handler. An intent the screen has no handler for is dropped.
 */
export function useRowIntent(
  handlers: Partial<Record<RowIntent, () => void>>,
  ready: boolean
): void {
  const done = useRef(false);
  const handlersRef = useRef(handlers);
  useEffect(() => {
    handlersRef.current = handlers;
  });

  useEffect(() => {
    if (!ready || done.current) return;
    done.current = true;
    const state: unknown = window.history.state;
    const intent = readIntent(state);
    if (intent === null) return;
    window.history.replaceState(withoutIntent(state), '');
    handlersRef.current[intent]?.();
  }, [ready]);
}
