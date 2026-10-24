/**
 * Webview bindings of the moony-keychain plugin (iOS and Android only).
 *
 * App code calls these only through `src/lib/tauri-api.ts`. Only `status`, `setPrivacyScreen` and
 * `setStatusBarStyle` are bound: `store`, `retrieve` and `remove` carry the database key, which
 * stays on the Rust side (`KeychainExt`), and the plugin's default permission set does not
 * grant them to the webview. The default set does not grant `setStatusBarStyle` either: the app's
 * mobile capability names `moony-keychain:allow-set-status-bar-style`.
 */
import { invoke } from '@tauri-apps/api/core';

/** The app's resolved theme, as `src/utils/theme.ts` names it. */
export type ResolvedTheme = 'light' | 'dark';

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

/**
 * Sets the status bar style outright: dark icons over the `light` theme, light icons over the
 * `dark` one. iOS replaces `preferredStatusBarStyle` on Tauri's view controller (the sampled style
 * lags behind a live theme switch on iOS 26 and is absent before it); Android sets
 * `isAppearanceLightStatusBars` (unverified on Android).
 */
export function setStatusBarStyle(theme: ResolvedTheme): Promise<void> {
  return invoke<void>('plugin:moony-keychain|set_status_bar_style', { theme });
}
