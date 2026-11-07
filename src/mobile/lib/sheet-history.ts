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

/**
 * Closes a sheet on back. A sheet may refuse (`false`: it asks first whether to drop what was
 * typed); it then keeps owning an entry, pushed again, so the next back asks again.
 */
export type SheetClose = () => boolean | void;

export interface SheetHistory {
  /**
   * A sheet opens and takes an entry. The returned function says the sheet has closed (however it
   * did); the entry is then taken off the history unless back has already done so.
   */
  open(close: SheetClose): () => void;
  /** A `popstate`: the newest sheet closes — unless it is only the echo of the stack's own step. */
  popstate(): void;
  /** How many entries the sheets hold in the history now (the open ones and those not yet taken off). */
  size(): number;
}

/** What the stack needs of the walk back after an unlock (`ReturnWalk`, `lock-return.ts`). */
export interface WalkPort {
  active(): boolean;
  whenDone(run: () => void): void;
}

export interface SheetHistoryOptions {
  /**
   * While the walk back after an unlock is under way, a sheet that opens waits to push its entry
   * until the walk has landed (the walk counts the entries it goes back over), and the walk's
   * `popstate` closes no sheet.
   */
  walk?: WalkPort;
  /** Called whenever the sheets held change (one opened, one taken off). */
  onChange?: () => void;
}

/** Key of the history state that marks the entry a sheet pushed. */
const MARK = 'moonySheet';

interface Entry {
  token: string;
  /** Closes the sheet on back; `null` once the sheet has closed (the entry waits to be removed). */
  close: SheetClose | null;
  /** Its history entry has been pushed (it waits while the walk back after an unlock runs). */
  pushed: boolean;
}

/** Whether a history state is the entry a sheet pushed. */
export function hasSheetMark(state: unknown): boolean {
  return markOf(state) !== undefined;
}

function markOf(state: unknown): unknown {
  return state !== null && typeof state === 'object'
    ? (state as Record<string, unknown>)[MARK]
    : undefined;
}

export function createSheetHistory(
  history: HistoryPort,
  options: SheetHistoryOptions = {}
): SheetHistory {
  const { walk, onChange } = options;
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
    const removed = entries.length > 0 && entries[entries.length - 1].close === null;
    let newest: Entry | undefined;
    let steps = 0;
    while (entries.length > 0 && entries[entries.length - 1].close === null) {
      const entry = entries.pop() as Entry;
      if (!entry.pushed) continue;
      newest ??= entry;
      steps++;
    }
    if (removed) onChange?.();
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

  /** Pushes the history entry of `entry`, over whatever entry is current. */
  const push = (entry: Entry) => {
    const current = history.state;
    const state = current !== null && typeof current === 'object' ? current : {};
    history.pushState({ ...state, [MARK]: entry.token }, '');
    entry.pushed = true;
  };

  /** Pushes, in order, the entries of the open sheets that waited for the walk. */
  const pushWaiting = () => {
    for (const entry of entries) if (!entry.pushed && entry.close !== null) push(entry);
  };

  return {
    open(close) {
      const entry: Entry = { token: String(++counter), close, pushed: false };
      entries.push(entry);
      if (walk?.active()) walk.whenDone(pushWaiting);
      else push(entry);
      onChange?.();
      return () => {
        entry.close = null;
        scheduleFlush();
      };
    },

    popstate() {
      // The walk back after an unlock: no sheet's entry was taken
      if (walk?.active()) return;
      if (ownSteps > 0) {
        ownSteps--;
        return;
      }
      // Back took the newest entry; the sheet's own release then clears any closed one beneath
      const entry = entries.pop();
      if (entry?.close && entry.close() === false) {
        // The sheet stays open (it asks first): it owns an entry again
        entries.push(entry);
        push(entry);
        return;
      }
      if (entry) onChange?.();
      scheduleFlush();
    },

    size() {
      return entries.length;
    },
  };
}
