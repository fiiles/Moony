import { useEffect, useRef, useState } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { pickDroppedFile } from './import-config';

interface UseFileDropOptions {
  /** Listen only while the drop zone is on screen. */
  enabled: boolean;
  /** Called with the first .csv/.txt path of a drop. */
  onFile: (path: string) => void;
  /** Called when something was dropped but no path was importable. */
  onRejected?: () => void;
}

/**
 * Files dropped on the window. Tauri handles native file drops itself
 * and never forwards them as HTML5 drag events, so the webview's own
 * `onDragDropEvent` is the only way to see them.
 *
 * Returns whether a drag is currently over the window. Outside Tauri (plain
 * browser preview) there is no webview to listen to; the zone then simply never
 * lights up and the "Choose file" button remains.
 */
export function useFileDrop({ enabled, onFile, onRejected }: UseFileDropOptions): boolean {
  const [isDragging, setIsDragging] = useState(false);

  // The listener is registered once per `enabled` flip; it reads the latest
  // callbacks from here instead of re-subscribing on every render.
  const handlers = useRef({ onFile, onRejected });
  useEffect(() => {
    handlers.current = { onFile, onRejected };
  });

  useEffect(() => {
    if (!enabled) return;

    let cancelled = false;
    let unlisten: (() => void) | undefined;

    try {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          const payload = event.payload;
          switch (payload.type) {
            case 'enter':
            case 'over':
              setIsDragging(true);
              break;
            case 'leave':
              setIsDragging(false);
              break;
            case 'drop': {
              setIsDragging(false);
              const path = pickDroppedFile(payload.paths);
              if (path) handlers.current.onFile(path);
              else handlers.current.onRejected?.();
              break;
            }
          }
        })
        .then((stop) => {
          // The effect may have been cleaned up while the listener was registering.
          if (cancelled) stop();
          else unlisten = stop;
        })
        .catch((error: unknown) => {
          console.error('Could not listen for dropped files:', error);
        });
    } catch {
      // No Tauri runtime (browser preview): nothing to listen to.
    }

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [enabled]);

  return enabled && isDragging;
}
