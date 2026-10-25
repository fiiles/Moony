import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { useAuth } from '@/hooks/use-auth';
import { RECEIVED_FILES_EVENT, receivedFilesApi } from '@/lib/tauri-api';
import { useImportSheet } from '@/mobile/hooks/use-import-sheet';
import { errorKind } from '@/mobile/lib/error-kind';
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
 *
 * `onPendingChange` tells the shell whether a shared file is on its way (`ReceivedQueue.pending`:
 * while locked and until the drain after the unlock answers, while a path waits, and while the
 * sheet shows one), so the one-time biometric offer waits for the import (`enrolmentOfferOpen`).
 * It is already up when the app unlocks: the offer's status is cached by the lock screen and is
 * decided on the first unlocked render, before any effect here could run.
 */
export function ReceivedFileHost({
  onPendingChange,
}: {
  onPendingChange?: (pending: boolean) => void;
}) {
  const { appStatus } = useAuth();
  const { openImport, open } = useImportSheet();
  const { t: tc } = useTranslation('common');
  const unlocked = appStatus === 'unlocked';

  const openImportRef = useRef(openImport);
  const onPendingRef = useRef(onPendingChange);
  // A shared file's import opens over whatever screen is up, so the account it leads to says a
  // neutral "Zpět" rather than naming a page back may not reach
  const backLabelRef = useRef(tc('buttons.back'));
  useEffect(() => {
    openImportRef.current = openImport;
    onPendingRef.current = onPendingChange;
    backLabelRef.current = tc('buttons.back');
  }, [openImport, onPendingChange, tc]);

  const [{ queue, drain, report }] = useState(() => {
    const queue = createReceivedQueue((path) => {
      openImportRef.current({ path, source: 'shared', from: backLabelRef.current });
    });
    const report = () => onPendingRef.current?.(queue.pending());
    const drain = createSerialDrain(async () => {
      let paths: string[] = [];
      try {
        paths = await receivedFilesApi.take();
      } catch (error) {
        // The kind only: the error's text can carry an Inbox path with the statement's name
        console.warn(`[received files] the shared files could not be read: ${errorKind(error)}`);
      }
      // A failed take answers with nothing: no file is on its way any more
      queue.offer(paths);
      report();
    });
    return { queue, drain, report };
  });

  // The sheet is taken while one is open and while the app is locked (a lock closes the sheet)
  const taken = open || !unlocked;
  useEffect(() => {
    if (taken) queue.sheetOpened();
    else queue.sheetClosed();
    report();
  }, [taken, queue, report]);

  useEffect(() => {
    if (!receivesFiles()) return;
    if (!unlocked) {
      // Rust keeps what is shared while locked: until the drain after the next unlock answers, a
      // file may be on its way
      queue.expect();
      report();
      return;
    }
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
  }, [unlocked, drain, queue, report]);

  return null;
}
