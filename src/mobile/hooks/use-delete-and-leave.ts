import { useRef, useState } from 'react';
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

/** What `leaveDeleted` moves the app with; the hook passes the router's and the query cache's. */
interface LeaveActions {
  back: () => void;
  /** Replaces the screen with `home`. */
  replace: (path: string) => void;
  removeQueries: (queryKey: QueryKey) => void;
}

/**
 * Leaves the screen of a record that is gone (the part of `useDeleteAndLeave` that moves). The
 * screen goes back only while it is still the record's own: a slow delete leaves the person time
 * to go elsewhere, and a `history.back()` then would throw them out of wherever they went. After
 * a short guard a screen that is still the record (a restored deep link, which had nothing to go
 * back to) is replaced by `home`. The record's own queries are removed in every case, and only
 * after `history.back()` has landed: removed while the screen is still up, a re-render would
 * rebuild them empty and flash the skeleton.
 */
export function leaveDeleted(
  { home, pathOf, keysOf }: Pick<DeleteAndLeaveOptions, 'home' | 'pathOf' | 'keysOf'>,
  { back, replace, removeQueries }: LeaveActions,
  id: string
): void {
  if (window.location.pathname === pathOf(id)) back();
  setTimeout(() => {
    if (window.location.pathname === pathOf(id)) replace(home);
    for (const queryKey of keysOf(id)) removeQueries(queryKey);
  }, DELETED_GUARD_MS);
}

/**
 * The delete flow of a detail screen (the loan's; the policy and the property delete the same
 * way): the confirm sheet's button calls `run(id)`, and when the record is gone the sheet closes
 * and the screen goes back (`leaveAfterSheet` → `leaveDeleted`); on a restored deep link, where
 * back has nowhere to go, the screen is replaced by `home` after a short guard, then the record's
 * own queries are removed.
 *
 * Own it on the screen, not in a menu or a row that the screen unmounts when the record drops out
 * of its list: the list refetches while the delete is still finishing (a mutation hook that
 * records a snapshot afterwards), and a callback of an unmounted observer is dropped, so the
 * screen would never leave. `deleting` is true from `run` until the screen has left (or the delete
 * failed): the screen keeps showing the record while it is (`useRetained`) instead of "no longer
 * exists".
 *
 * `run` starts one delete: a second call while it is under way (a double tap on the confirm
 * button, before the button shows its pending state) does nothing, so the record is never deleted
 * twice and the screen never leaves twice. A failed delete lets the next call through.
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
  // A ref, set synchronously: state would still read false for a second tap in the same frame
  const started = useRef(false);

  const leave = (id: string) =>
    leaveDeleted(
      { home, pathOf, keysOf },
      {
        back,
        replace: (path) => setLocation(path, { replace: true }),
        removeQueries: (queryKey) => queryClient.removeQueries({ queryKey }),
      },
      id
    );

  const run = (id: string) => {
    if (started.current) return;
    started.current = true;
    setDeleting(true);
    // The failure is already shown (the mutation's own toast); the screen is as it was
    remove(id).then(
      () => leaveAfterSheet(closeSheet, () => leave(id)),
      () => {
        started.current = false;
        setDeleting(false);
      }
    );
  };

  return { deleting, run };
}
