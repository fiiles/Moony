/**
 * The open sheets in the order they opened (audit C12): a sheet that another one covers hides its
 * drag handle, so a sheet opened over a sheet shows one grabber, not two. A tiny external store,
 * read with `useSyncExternalStore` (`Sheet.tsx`); ids are the sheets' `useId`.
 */
export interface SheetStack {
  /** Puts the sheet on top (once; a second open of the same id keeps its place). */
  open: (id: string) => void;
  close: (id: string) => void;
  /** True when a sheet opened after this one is still open. */
  covered: (id: string) => boolean;
  subscribe: (listener: () => void) => () => void;
  /** Changes whenever the stack does (the store's snapshot). */
  version: () => number;
}

export function createSheetStack(): SheetStack {
  let ids: string[] = [];
  let version = 0;
  const listeners = new Set<() => void>();
  const changed = () => {
    version += 1;
    for (const listener of listeners) listener();
  };
  return {
    open(id) {
      if (ids.includes(id)) return;
      ids = [...ids, id];
      changed();
    },
    close(id) {
      if (!ids.includes(id)) return;
      ids = ids.filter((other) => other !== id);
      changed();
    },
    covered(id) {
      const at = ids.indexOf(id);
      return at >= 0 && at < ids.length - 1;
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    version: () => version,
  };
}

/** The phone's one stack. */
export const sheetStack = createSheetStack();
