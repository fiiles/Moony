/**
 * The history entries of open sheets (spec §10.3): each open sheet owns one entry, so Android back
 * closes the newest sheet and the screen under it stays where it was. Sheets stack (the category
 * picker over the transaction sheet, a confirmation over either), so the entries are a stack:
 * back closes the newest sheet only, and a sheet that closes by other means takes its entry off
 * without disturbing the sheets beneath. Pure: `window.history` arrives as a port.
 */

/** What the stack needs of `window.history`. */
export interface HistoryPort {
  readonly state: unknown;
  pushState(state: unknown, unused: string): void;
  go(delta: number): void;
}

export interface SheetHistory {
  /**
   * A sheet opens and takes an entry. The returned function says the sheet has closed (however it
   * did); the entry is then taken off the history unless back has already done so.
   */
  open(close: () => void): () => void;
  /** A `popstate`: the newest sheet closes — unless it is only the echo of the stack's own step. */
  popstate(): void;
}

/** Key of the history state that marks the entry a sheet pushed. */
const MARK = 'moonySheet';

interface Entry {
  token: string;
  /** Closes the sheet on back; `null` once the sheet has closed (the entry waits to be removed). */
  close: (() => void) | null;
}

function markOf(state: unknown): unknown {
  return state !== null && typeof state === 'object'
    ? (state as Record<string, unknown>)[MARK]
    : undefined;
}

export function createSheetHistory(history: HistoryPort): SheetHistory {
  /** One per pushed entry, oldest first. */
  const entries: Entry[] = [];
  let counter = 0;
  /** Steps the stack took itself (`go`) whose `popstate` has not arrived yet. */
  let ownSteps = 0;
  let flushScheduled = false;

  /**
   * Takes off the closed entries from the top in one step. It runs after the commit in which the
   * sheets closed, so sheets that close together (a confirmed delete closes two) share one step
   * whatever order they report in. When the newest of them is no longer the history's current
   * entry the app has navigated on, and the history stays as it is.
   */
  const flush = () => {
    flushScheduled = false;
    let newest: Entry | undefined;
    let steps = 0;
    while (entries.length > 0 && entries[entries.length - 1].close === null) {
      newest ??= entries[entries.length - 1];
      entries.pop();
      steps++;
    }
    if (newest && markOf(history.state) === newest.token) {
      ownSteps++;
      history.go(-steps);
    }
  };

  const scheduleFlush = () => {
    if (flushScheduled) return;
    flushScheduled = true;
    queueMicrotask(flush);
  };

  return {
    open(close) {
      const entry: Entry = { token: String(++counter), close };
      entries.push(entry);
      const current = history.state;
      const state = current !== null && typeof current === 'object' ? current : {};
      history.pushState({ ...state, [MARK]: entry.token }, '');
      return () => {
        entry.close = null;
        scheduleFlush();
      };
    },

    popstate() {
      if (ownSteps > 0) {
        ownSteps--;
        return;
      }
      // Back took the newest entry; the sheet's own release then clears any closed one beneath
      entries.pop()?.close?.();
      scheduleFlush();
    },
  };
}
