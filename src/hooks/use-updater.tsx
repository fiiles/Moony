/**
 * The one updater for the whole app.
 *
 * `UpdaterProvider` is mounted once (App.tsx). It owns the Tauri updater calls
 * and every piece of update state; the header badge, the "update available"
 * dialog, the About modal and the Settings card only read from it, so there is
 * a single dismiss and a single install flow.
 *
 * Automatic check: at most once per app session, only after the user has
 * unlocked Moony, and only while the device-level setting "Check for updates
 * automatically" is on (`moony-auto-update-check`, default on). A manual
 * `check()` counts as that session's check.
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { check, Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { useAuth } from '@/hooks/use-auth';
import { readAutoUpdateCheck, writeAutoUpdateCheck } from '@/utils/auto-update';

/** Delay after unlock before the automatic check, so it never competes with startup work. */
const AUTO_CHECK_DELAY_MS = 5000;

export interface UpdateProgress {
  downloaded: number;
  contentLength: number | null;
  percentage: number;
}

export interface UpdateInfo {
  version: string;
  date: string | null;
  body: string | null;
}

export type UpdaterStatus =
  /** No check has run yet in this session. */
  | 'idle'
  | 'checking'
  | 'upToDate'
  /** A newer version exists; see `update`. */
  | 'available'
  /** Downloading / installing; the app relaunches when it finishes. */
  | 'installing'
  /** The last check failed (offline, no release published yet, ...). */
  | 'checkFailed';

export interface UpdaterContextValue {
  status: UpdaterStatus;
  /** The available update, or null. Kept after `dismiss()` so the badge can still offer it. */
  update: UpdateInfo | null;
  progress: UpdateProgress | null;
  /** The last install attempt failed (the dialog shows a localized message). */
  installFailed: boolean;
  /** Whether the shared "update available" dialog is open. */
  promptOpen: boolean;
  /** Device-level setting: run the automatic check after unlock. */
  autoCheck: boolean;
  setAutoCheck: (enabled: boolean) => void;
  /** Manual check; the result lands in `status` and does not open the dialog. */
  check: () => Promise<void>;
  /** Download, install and relaunch; opens the dialog to show progress. */
  install: () => Promise<void>;
  /** Close the dialog (the update stays available through the badge). */
  dismiss: () => void;
  /** Open the dialog for the available update. */
  showPrompt: () => void;
}

const UpdaterContext = createContext<UpdaterContextValue | null>(null);

export function UpdaterProvider({ children }: { children: ReactNode }) {
  const { user } = useAuth();
  const [status, setStatus] = useState<UpdaterStatus>('idle');
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [installFailed, setInstallFailed] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [autoCheck, setAutoCheckState] = useState<boolean>(() => readAutoUpdateCheck());

  // The plugin handle is not serializable state; keep it out of React state.
  const handleRef = useRef<Update | null>(null);
  const busyRef = useRef(false);
  const checkedRef = useRef(false);

  const runCheck = useCallback(async (promptWhenFound: boolean) => {
    if (busyRef.current) return;
    busyRef.current = true;
    checkedRef.current = true;
    setStatus('checking');

    try {
      const result = await check();

      if (result) {
        handleRef.current = result;
        setUpdate({
          version: result.version,
          date: result.date ?? null,
          body: result.body ?? null,
        });
        setStatus('available');
        if (promptWhenFound) setPromptOpen(true);
      } else {
        handleRef.current = null;
        setUpdate(null);
        setStatus('upToDate');
      }
    } catch (err) {
      // Expected when offline or before the first release exists: log it, never toast it.
      console.warn('Update check failed (this is normal if no releases exist):', err);
      setStatus(handleRef.current ? 'available' : 'checkFailed');
    } finally {
      busyRef.current = false;
    }
  }, []);

  const manualCheck = useCallback(() => runCheck(false), [runCheck]);

  const install = useCallback(async () => {
    const handle = handleRef.current;
    if (!handle || busyRef.current) return;
    busyRef.current = true;
    setInstallFailed(false);
    setStatus('installing');
    setPromptOpen(true);
    setProgress({ downloaded: 0, contentLength: null, percentage: 0 });

    try {
      let downloaded = 0;
      let contentLength: number | null = null;

      await handle.downloadAndInstall((event) => {
        switch (event.event) {
          case 'Started':
            contentLength = event.data.contentLength ?? null;
            break;
          case 'Progress': {
            downloaded += event.data.chunkLength;
            const percentage = contentLength ? Math.round((downloaded / contentLength) * 100) : 0;
            setProgress({ downloaded, contentLength, percentage });
            break;
          }
          case 'Finished':
            setProgress({ downloaded, contentLength, percentage: 100 });
            break;
        }
      });

      await relaunch();
    } catch (err) {
      console.error('Failed to download/install update:', err);
      setInstallFailed(true);
      setProgress(null);
      setStatus('available');
    } finally {
      busyRef.current = false;
    }
  }, []);

  const dismiss = useCallback(() => {
    setPromptOpen(false);
    setInstallFailed(false);
  }, []);

  const showPrompt = useCallback(() => setPromptOpen(true), []);

  const setAutoCheck = useCallback((enabled: boolean) => {
    setAutoCheckState(enabled);
    writeAutoUpdateCheck(enabled);
  }, []);

  // Automatic check: after unlock only, once per session, only while enabled.
  const unlocked = user !== null;
  useEffect(() => {
    if (!unlocked || !autoCheck || checkedRef.current) return;
    const timer = setTimeout(() => void runCheck(true), AUTO_CHECK_DELAY_MS);
    return () => clearTimeout(timer);
  }, [unlocked, autoCheck, runCheck]);

  const value = useMemo<UpdaterContextValue>(
    () => ({
      status,
      update,
      progress,
      installFailed,
      promptOpen,
      autoCheck,
      setAutoCheck,
      check: manualCheck,
      install,
      dismiss,
      showPrompt,
    }),
    [
      status,
      update,
      progress,
      installFailed,
      promptOpen,
      autoCheck,
      setAutoCheck,
      manualCheck,
      install,
      dismiss,
      showPrompt,
    ]
  );

  return <UpdaterContext.Provider value={value}>{children}</UpdaterContext.Provider>;
}

// eslint-disable-next-line react-refresh/only-export-components -- context hook lives with its provider
export function useUpdater(): UpdaterContextValue {
  const context = useContext(UpdaterContext);
  if (!context) {
    throw new Error('useUpdater must be used within an UpdaterProvider');
  }
  return context;
}
