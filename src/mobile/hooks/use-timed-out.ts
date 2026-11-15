import { useEffect, useState } from 'react';

/**
 * True once `waiting` has stayed true for `ms` without a break; false again as soon as it is not.
 * A screen that would otherwise show its skeleton for ever (audit E2) turns it into `LoadError`
 * with a retry.
 */
export function useTimedOut(waiting: boolean, ms: number): boolean {
  const [timedOut, setTimedOut] = useState(false);
  useEffect(() => {
    if (!waiting) return;
    const timer = window.setTimeout(() => setTimedOut(true), ms);
    return () => {
      window.clearTimeout(timer);
      setTimedOut(false);
    };
  }, [waiting, ms]);
  return waiting && timedOut;
}
