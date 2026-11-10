import { useEffect, useState } from 'react';
import type { PairingView } from '@shared/schema';

/** True once the pairing's `expiresAt` (unix seconds) has passed: the poll stops there, so the UI must notice. */
export function usePairingExpired(view: PairingView | undefined): boolean {
  const expiresAt = view?.expiresAt;
  // The `expiresAt` the timer fired for: a new pairing (another `expiresAt`) starts unexpired.
  const [firedFor, setFiredFor] = useState<number | null>(null);
  useEffect(() => {
    if (expiresAt === undefined) return;
    const timer = setTimeout(
      () => setFiredFor(expiresAt),
      Math.max(0, expiresAt * 1000 - Date.now())
    );
    return () => clearTimeout(timer);
  }, [expiresAt]);
  return expiresAt !== undefined && firedFor === expiresAt;
}
