/**
 * Webview bindings of the moony-keychain plugin (iOS and Android only).
 *
 * App code calls these only through `src/lib/tauri-api.ts`. Only `status` and
 * `setPrivacyScreen` are bound: `store`, `retrieve` and `remove` carry the database key, which
 * stays on the Rust side (`KeychainExt`), and the plugin's default permission set does not
 * grant them to the webview.
 */
import { invoke } from '@tauri-apps/api/core';

/** Which biometry the device offers. */
export type BiometryType = 'faceId' | 'touchId' | 'biometric' | 'none';

/** Whether a biometry-protected secret can be stored and read right now. */
export interface KeychainStatus {
  available: boolean;
  biometryType: BiometryType;
}

/** The `code` of a rejected keychain call. */
export type KeychainErrorCode = 'invalidated' | 'cancelled' | 'unavailable' | 'failed';

export function status(): Promise<KeychainStatus> {
  return invoke<KeychainStatus>('plugin:moony-keychain|status');
}

/** Blur the app in the app switcher / recents (on by default). */
export function setPrivacyScreen(enabled: boolean): Promise<void> {
  return invoke<void>('plugin:moony-keychain|set_privacy_screen', { enabled });
}
