/**
 * The statements the system opened in Moony (spec §8.6), on their way to the import sheet. Pure:
 * no React and no Tauri, so the rules are testable — the host (`ReceivedFileHost`) feeds it from
 * the Rust queue and tells it when the sheet is taken or free.
 */

export interface ReceivedQueue {
  /**
   * Paths that came from `take()`. A path that is waiting, or whose sheet is still open, is not
   * taken twice, however often it is offered (the drain at unlock and the "received-files" event
   * can hand over the same file). Once its sheet has closed the copy is gone (the sheet deletes
   * it), so the same path offered again is a new file: iOS names the copy of a bank's weekly
   * "Transactions.csv" the same every time.
   */
  offer(paths: readonly string[]): void;
  /**
   * The sheet is taken by something else — one the person opened, or a locked app. Nothing is
   * delivered until `sheetClosed()`.
   */
  sheetOpened(): void;
  /** The sheet is free again; the path it showed is done, and the next waiting one is delivered. */
  sheetClosed(): void;
}

/**
 * One path at a time: a path opens the import sheet, and the next waits until the host reports the
 * sheet closed (`sheetClosed`), so a second shared file never replaces the import in front of the
 * person, and a path is never handed to a sheet that is already open (which would ignore it).
 */
export function createReceivedQueue(deliver: (path: string) => void): ReceivedQueue {
  // Waiting, or on the sheet now: what a repeated offer must not queue again
  const outstanding = new Set<string>();
  const waiting: string[] = [];
  let showing: string | null = null;
  let taken = false;

  const pump = () => {
    if (taken) return;
    const next = waiting.shift();
    if (next === undefined) return;
    taken = true;
    showing = next;
    deliver(next);
  };

  return {
    offer(paths) {
      for (const path of paths) {
        if (outstanding.has(path)) continue;
        outstanding.add(path);
        waiting.push(path);
      }
      pump();
    },
    sheetOpened() {
      taken = true;
    },
    sheetClosed() {
      taken = false;
      if (showing !== null) {
        outstanding.delete(showing);
        showing = null;
      }
      pump();
    },
  };
}

/**
 * Runs `run` one at a time. The first `take()` of a run also sweeps the Inbox folders and keeps
 * only the paths it hands out, so two takes must never overlap: a request while a run is going
 * schedules exactly one more run after it, however many arrive. A failed run is the run's to
 * report; it never stops the next.
 */
export function createSerialDrain(run: () => Promise<void>): { request(): void } {
  let running = false;
  let again = false;

  const loop = async () => {
    running = true;
    do {
      again = false;
      try {
        await run();
      } catch {
        // The next request tries again
      }
    } while (again);
    running = false;
  };

  return {
    request() {
      if (running) {
        again = true;
        return;
      }
      void loop();
    },
  };
}
