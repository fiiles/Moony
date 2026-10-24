/**
 * The phone's status bar follows the app's theme through an explicit native style.
 *
 * Left to itself the OS samples the page under the bar: iOS 26 keeps the style it sampled before a
 * live theme switch repainted (light text over a light page until the next scroll; checked on
 * 26.3, earlier iOS was not verified). The keychain plugin's `set_status_bar_style` command sets
 * the style outright (`keychainApi.setStatusBarStyle`); `StatusBarStyle` in `MobileApp.tsx` calls
 * this when the resolved theme changes.
 */
import { keychainApi } from '@/lib/tauri-api';
import type { ResolvedTheme } from '@/utils/theme';

function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Asks the native shell for the status bar style of `theme` (`light` gets dark icons, `dark` light
 * ones). A no-op outside Tauri; a rejection (a platform without the command, a missing grant) is
 * swallowed, as the page itself already follows `data-theme`.
 */
export async function applyStatusBarStyle(theme: ResolvedTheme): Promise<void> {
  if (!inTauri()) return;
  try {
    await keychainApi.setStatusBarStyle(theme);
  } catch {
    // Missing command or permission: the OS falls back to sampling the page.
  }
}
