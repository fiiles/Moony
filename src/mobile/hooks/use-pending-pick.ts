import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';

/** How long a pick may save before the sheet gives up waiting and says so (audit E3). */
export const PICK_TIMEOUT_MS = 12_000;

/**
 * A picker whose tap saves before the sheet closes (the language, the main currency): the picked
 * value while its save runs, for `PickRow`'s `pending` and `dimmed`, and `run`, which starts one
 * save at a time. A finished save hands over to `onSaved` (the sheet closes); a failed one has said
 * so in its own toast and lets the person pick again. A save that has not finished after
 * `PICK_TIMEOUT_MS` shows `timeoutMessage` and frees the rows; should it still succeed later,
 * `onSaved` follows then.
 */
export function usePendingPick<T>(timeoutMessage: string) {
  const [pending, setPending] = useState<T | null>(null);
  const busy = useRef(false);
  // The run whose save the rows wait on; a save that outlived its timeout must not end a newer one
  const current = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  const run = (value: T, save: () => Promise<unknown>, onSaved: () => void) => {
    if (busy.current) return;
    busy.current = true;
    setPending(value);
    const id = ++current.current;
    const settle = () => {
      if (id !== current.current || !busy.current) return;
      clearTimeout(timer.current);
      busy.current = false;
      setPending(null);
    };
    timer.current = setTimeout(() => {
      settle();
      toast.error(timeoutMessage);
    }, PICK_TIMEOUT_MS);
    save().then(
      () => {
        settle();
        onSaved();
      },
      // The mutation's own toast has said what went wrong
      settle
    );
  };

  return { pending, run };
}
