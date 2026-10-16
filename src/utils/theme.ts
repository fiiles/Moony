import type { ThemePreference } from '@shared/schema';
import {
  readSetting,
  writeSetting,
  type ReadableStorage,
  type WritableStorage,
} from './device-storage';

/**
 * The theme choice is a device setting (`localStorage`): it must be readable before the encrypted
 * profile is unlocked, so the login screen and the first paint already follow it.
 * `public/theme-boot.js` repeats the read and the resolution in plain ES5 and must stay in step
 * with this module (its test covers the shared cases).
 */
export type { ThemePreference };
export type ResolvedTheme = 'light' | 'dark';

export const THEME_KEY = 'moony-theme';

const PREFERENCES: readonly ThemePreference[] = ['system', 'light', 'dark'];

export function isThemePreference(v: unknown): v is ThemePreference {
  return PREFERENCES.includes(v as ThemePreference);
}

/** An explicit choice wins; `system` follows the OS appearance. */
export function resolveTheme(pref: ThemePreference, systemDark: boolean): ResolvedTheme {
  if (pref === 'system') return systemDark ? 'dark' : 'light';
  return pref;
}

/** Missing, unknown or unreadable storage all mean the default, `system`. */
export function readStoredPreference(storage?: ReadableStorage): ThemePreference {
  const stored = readSetting(THEME_KEY, storage);
  return isThemePreference(stored) ? stored : 'system';
}

export function writeStoredPreference(pref: ThemePreference, storage?: WritableStorage): void {
  writeSetting(THEME_KEY, pref, storage);
}
