/**
 * Which frontend shell to render: the phone shell (`src/mobile/`) or the desktop `App`.
 *
 * Decided at startup from build-time variables, never at render time. `TAURI_ENV_PLATFORM` is set by
 * the Tauri CLI for the frontend build as the OS part of the target triple (`ios`, `android`,
 * `androideabi` on 32-bit ARM Android, `darwin`, `windows`, `linux`); `VITE_MOONY_SHELL`
 * (`mobile` | `desktop`) overrides it so either shell can be opened in a plain browser tab or
 * on the other platform. Any other value of the override is ignored.
 */
export interface ShellEnv {
  TAURI_ENV_PLATFORM?: string;
  VITE_MOONY_SHELL?: string;
}

export function isMobileShell(env: ShellEnv): boolean {
  if (env.VITE_MOONY_SHELL === 'mobile') return true;
  if (env.VITE_MOONY_SHELL === 'desktop') return false;
  const platform = env.TAURI_ENV_PLATFORM ?? '';
  return platform === 'ios' || platform.startsWith('android');
}

/**
 * Whether a menu, popover or select list owns a history entry while open (`withPhoneHistory`), so
 * that Android's back button closes it: the Android phone shell only. iOS has no back button, and
 * an entry there would only shift the history the app's own back walks count on.
 */
export function menusHoldHistory(env: ShellEnv): boolean {
  return isMobileShell(env) && (env.TAURI_ENV_PLATFORM ?? '').startsWith('android');
}
