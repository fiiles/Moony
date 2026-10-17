import { useCallback } from 'react';
import { useLocation } from 'wouter';

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
