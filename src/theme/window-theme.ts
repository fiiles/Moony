/**
 * The native side of the theme: the window chrome, the window background and the OS appearance.
 *
 * Every Tauri call is guarded and its failure swallowed. In a plain browser (no dev bridge) the
 * API throws on first use, and a missing capability or a platform without the call (for example
 * `setTheme` on some Linux setups) must never break the UI, which already follows `data-theme`.
 */
import { getCurrentWindow } from '@tauri-apps/api/window';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { token } from '@/lib/tokens';
import type { ResolvedTheme, ThemePreference } from '@/utils/theme';

const DARK_QUERY = '(prefers-color-scheme: dark)';

function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function hasMatchMedia(): boolean {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function';
}

/** Runs a Tauri call outside Tauri as a no-op; a throw or a rejection is swallowed. */
async function guarded(call: () => Promise<unknown>): Promise<void> {
  if (!inTauri()) return;
  try {
    await call();
  } catch {
    // Missing permission or unsupported platform: the CSS theme is already applied.
  }
}

/** Whether the webview currently reports a dark OS appearance (`false` without `matchMedia`). */
export function systemPrefersDark(): boolean {
  return hasMatchMedia() && window.matchMedia(DARK_QUERY).matches;
}

/**
 * Sets the native window theme (title bar, native dialogs). `system` passes `null`, which hands
 * the appearance back to the OS; the override is app-wide on macOS and Linux, so an explicit
 * choice must be cleared this way.
 */
export async function applyWindowTheme(pref: ThemePreference): Promise<void> {
  await guarded(() => getCurrentWindow().setTheme(pref === 'system' ? null : pref));
}

/**
 * Paints the window and the webview layer in the canvas color, so a resize or an overscroll never
 * flashes the other theme. Call after `data-theme` is stamped and the token cache is reset, or the
 * old theme's canvas is read. The color comes from the stylesheet, `resolved` only names the
 * theme just applied.
 */
export async function applyWindowBackground(_resolved: ResolvedTheme): Promise<void> {
  await guarded(() => getCurrentWebviewWindow().setBackgroundColor(token('canvas')));
}

/** The window starts hidden (no flash of the wrong theme); this shows it after the first stamp. */
export async function showWindow(): Promise<void> {
  await guarded(() => getCurrentWindow().show());
}

/**
 * Calls `cb` when the OS appearance may have changed: the `prefers-color-scheme` media query and,
 * in Tauri, the window's theme event. The caller re-reads the media query itself. Returns the
 * unsubscribe, which also drops the Tauri listener when it is still being registered.
 */
export function onSystemThemeChange(cb: () => void): () => void {
  const cleanups: Array<() => void> = [];

  if (hasMatchMedia()) {
    const query = window.matchMedia(DARK_QUERY);
    query.addEventListener('change', cb);
    cleanups.push(() => query.removeEventListener('change', cb));
  }

  if (inTauri()) {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    try {
      getCurrentWindow()
        .onThemeChanged(() => cb())
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        })
        .catch(() => undefined);
    } catch {
      // The listener is a bonus on top of the media query.
    }
    cleanups.push(() => {
      cancelled = true;
      try {
        unlisten?.();
      } catch {
        // Already gone with the window.
      }
    });
  }

  return () => cleanups.forEach((cleanup) => cleanup());
}
