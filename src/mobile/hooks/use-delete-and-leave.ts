import { useState } from 'react';
import { useQueryClient, type QueryKey } from '@tanstack/react-query';
import { useLocation } from 'wouter';
import { useBack } from '@/mobile/hooks/use-back';
import { leaveAfterSheet } from '@/mobile/hooks/use-leave-after-sheet';

/** How long after leaving a deleted record the screen may still be it (a restored deep link). */
const DELETED_GUARD_MS = 250;

interface DeleteAndLeaveOptions {
  /** The tab root the screen goes back to, and replaces a restored deep link with. */
  home: string;
  /** The path of the record's own screen (`loanPath`). */
  pathOf: (id: string) => string;
  /** The record's own query keys: dropped once the screen has left, so no ghost of it stays. */
  keysOf: (id: string) => readonly QueryKey[];
  /** Deletes the record: resolves once it is gone (`mutation.mutateAsync`), rejects when it failed. */
  remove: (id: string) => Promise<unknown>;
  /** Closes the confirm sheet; its history entry is off the stack before the screen goes back. */
  closeSheet: () => void;
}

/**
 * The delete flow of a detail screen (the loan's; the policy and the property delete the same
 * way): the confirm sheet's button calls `run(id)`, and when the record is gone the sheet closes
 * and the screen goes back (`leaveAfterSheet` → `useBack`); on a restored deep link, where back
 * has nowhere to go, the screen is replaced by `home` after a short guard, then the record's own
 * queries are removed.
 *
 * Own it on the screen, not in a menu or a row that the screen unmounts when the record drops out
 * of its list: the list refetches while the delete is still finishing (a mutation hook that
 * records a snapshot afterwards), and a callback of an unmounted observer is dropped, so the
 * screen would never leave. `deleting` is true from `run` until the screen has left (or the delete
 * failed): the screen keeps showing the record while it is (`useRetained`) instead of "no longer
 * exists".
 */
export function useDeleteAndLeave({
  home,
  pathOf,
  keysOf,
  remove,
  closeSheet,
}: DeleteAndLeaveOptions): { deleting: boolean; run: (id: string) => void } {
  const [, setLocation] = useLocation();
  const queryClient = useQueryClient();
  const back = useBack(home);
  const [deleting, setDeleting] = useState(false);

  // history.back() lands asynchronously: the record's queries are removed after it, since removed
  // while the screen is still up a re-render would rebuild them empty and flash the skeleton
  const leave = (id: string) => {
    back();
    window.setTimeout(() => {
      if (window.location.pathname === pathOf(id)) setLocation(home, { replace: true });
      for (const queryKey of keysOf(id)) queryClient.removeQueries({ queryKey });
    }, DELETED_GUARD_MS);
  };

  const run = (id: string) => {
    setDeleting(true);
    // The failure is already shown (the mutation's own toast); the screen is as it was
    remove(id).then(
      () => leaveAfterSheet(closeSheet, () => leave(id)),
      () => setDeleting(false)
    );
  };

  return { deleting, run };
}
