/**
 * Device-level settings (auto-lock, update checks) live in `localStorage`, not in
 * the encrypted profile: they apply to this machine only and must be readable
 * before the profile is. Storage can be missing or throw (blocked site data,
 * tests), so every access goes through these helpers and falls back to defaults.
 */

export type ReadableStorage = Pick<Storage, 'getItem'>;
export type WritableStorage = Pick<Storage, 'setItem'>;

/** `localStorage` when it is usable, otherwise `undefined`. */
export function deviceStorage(): Storage | undefined {
  try {
    return typeof localStorage === 'undefined' ? undefined : localStorage;
  } catch {
    return undefined;
  }
}

export function readSetting(
  key: string,
  storage: ReadableStorage | undefined = deviceStorage()
): string | null {
  try {
    return storage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

export function writeSetting(
  key: string,
  value: string,
  storage: WritableStorage | undefined = deviceStorage()
): void {
  try {
    storage?.setItem(key, value);
  } catch {
    // Storage can be blocked or full; the setting then only lasts for this session.
  }
}
