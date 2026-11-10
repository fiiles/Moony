import {
  Format,
  cancel,
  checkPermissions,
  openAppSettings,
  requestPermissions,
  scan,
} from '@tauri-apps/plugin-barcode-scanner';
import { isScanCancelled } from './sync-scan';

/**
 * The camera's QR scanner (`@tauri-apps/plugin-barcode-scanner`, mobile only): the only importer of
 * the plugin. Sync pairing scans the other device's pairing code and the Recovery Kit's QR with it.
 *
 * The scan is `windowed`: the camera runs under a transparent webview and the app draws its own
 * frame and Cancel over it (`ScanOverlay`, which also makes the page transparent). The plugin's
 * full-screen view on iOS covers the webview with no way back (its 2.5.3 source inserts the camera
 * above the webview and offers no button), so it is never used.
 */

export type ScanOutcome =
  { kind: 'code'; text: string } | { kind: 'cancelled' } | { kind: 'denied' };

/**
 * A cancel that came before the camera opened (while the permission prompt was up): the plugin has
 * no scan to cancel then, so `scanQrCode` checks this just before it starts one. Reset by every
 * new `scanQrCode`.
 */
let cancelRequested = false;

/**
 * Asks for the camera when it has not been asked yet, then scans one QR code. `denied`: the camera
 * is refused (Settings can allow it, `openCameraSettings`); `cancelled`: `cancelQrScan` ended it.
 * Any other failure (no camera, as on the simulator) rejects.
 */
export async function scanQrCode(): Promise<ScanOutcome> {
  cancelRequested = false;
  let permission = await checkPermissions();
  // Not asked yet ('prompt', and Android's 'prompt-with-rationale'): the system asks now
  if (permission !== 'granted' && permission !== 'denied') {
    permission = await requestPermissions();
  }
  if (permission !== 'granted') return { kind: 'denied' };
  // Cancelled, unmounted or sent to the background during the prompt: no camera without its overlay
  if (cancelRequested) return { kind: 'cancelled' };
  try {
    const scanned = await scan({ windowed: true, formats: [Format.QRCode] });
    return { kind: 'code', text: scanned.content };
  } catch (error) {
    if (isScanCancelled(error)) return { kind: 'cancelled' };
    throw error;
  }
}

/**
 * Ends a running scan (its promise answers `cancelled`), or one still waiting for the permission
 * prompt (it answers `cancelled` without opening the camera); nothing happens when none runs.
 */
export async function cancelQrScan(): Promise<void> {
  cancelRequested = true;
  try {
    await cancel();
  } catch {
    // No scanner here (desktop, browser) or nothing to cancel: either way no camera is open.
  }
}

/** The app's page in the system Settings, where a refused camera can be allowed. */
export async function openCameraSettings(): Promise<void> {
  await openAppSettings();
}
