import { useCallback } from 'react';
import { useLocation } from 'wouter';
import { useHistoryState } from 'wouter/use-browser-location';
import { backLabel } from '@/mobile/lib/back-label';

/**
 * Back from a pushed screen: pops the history entry the push added (as Android back does), or
 * replaces the screen with `fallback` when it was opened without one (a restored deep link).
 */
export function useBack(fallback: string): () => void {
  const [, setLocation] = useLocation();
  return useCallback(() => {
    if (window.history.length > 1) window.history.back();
    else setLocation(fallback, { replace: true });
  }, [fallback, setLocation]);
}

/**
 * The back button's label: the screen the push left when it passed one (`backState`, a push that
 * leaves its tab), else the screen's own `fallback` (its parent's name).
 */
export function useBackLabel(fallback: string): string {
  return backLabel(useHistoryState<unknown>(), fallback);
}
