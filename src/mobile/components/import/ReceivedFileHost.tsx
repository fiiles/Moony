import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useAuth } from '@/hooks/use-auth';
import { RECEIVED_FILES_EVENT, receivedFilesApi } from '@/lib/tauri-api';
import { useImportSheet } from '@/mobile/hooks/use-import-sheet';
import { createReceivedQueue, createSerialDrain } from '@/mobile/lib/received-queue';

/** Only the iOS shell registers the received-files commands (`#[cfg(mobile)]` in Rust). */
const receivesFiles = () => import.meta.env.TAURI_ENV_PLATFORM === 'ios';

/**
 * Opens a statement the system handed to Moony (the share sheet, "Open in Moony", spec §8.6) in
 * the import sheet. Draws nothing; mounted once in the shell, inside `ImportSheetProvider`.
 *
 * Rust queues every file that arrives while the app is locked or not yet running and keeps it
 * until `take`, so nothing is taken while locked: the first drain happens as soon as the app is
 * unlocked (it also sweeps the Inbox folders, so it comes before the person can pick a file), and
 * "received-files" asks for another while unlocked. Drains run one at a time (`createSerialDrain`),
 * and what they return goes through `createReceivedQueue`, which delivers each path once and one
 * at a time: while a sheet is open (this one's or one the person opened) or the app is locked, a
 * path waits and opens when the sheet closes.
 *
 * The host stays mounted through a lock and only works while unlocked, so the queue — the paths
 * that are taken but not yet shown, and the one on the sheet — outlives every unlock: a path is
 * neither lost with a screen nor shown twice.
 */
export function ReceivedFileHost() {
  const { appStatus } = useAuth();
  const { openImport, open } = useImportSheet();
  const unlocked = appStatus === 'unlocked';

  const openImportRef = useRef(openImport);
  useEffect(() => {
    openImportRef.current = openImport;
  }, [openImport]);

  const [{ queue, drain }] = useState(() => {
    const queue = createReceivedQueue((path) => {
      openImportRef.current({ path, source: 'shared' });
    });
    const drain = createSerialDrain(async () => {
      try {
        queue.offer(await receivedFilesApi.take());
      } catch (error) {
        console.warn('[received files] the shared files could not be read', error);
      }
    });
    return { queue, drain };
  });

  // The sheet is taken while one is open and while the app is locked (a lock closes the sheet)
  const taken = open || !unlocked;
  useEffect(() => {
    if (taken) queue.sheetOpened();
    else queue.sheetClosed();
  }, [taken, queue]);

  useEffect(() => {
    if (!unlocked || !receivesFiles()) return;
    let current = true;
    let unlisten: (() => void) | undefined;
    // Listen first, then drain: a file that arrives in between is still seen by the drain, and
    // one that arrives after it by the event
    listen(RECEIVED_FILES_EVENT, () => drain.request())
      .then((stop) => {
        if (current) unlisten = stop;
        else stop();
      })
      .catch(() => undefined)
      .finally(() => {
        if (current) drain.request();
      });
    return () => {
      current = false;
      unlisten?.();
    };
  }, [unlocked, drain]);

  return null;
}
