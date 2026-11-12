const KNOWN_PLATFORMS = new Set(['macos', 'windows', 'linux', 'ios', 'android']);

/** The i18n key of a device's platform label; unknown platforms read as "other". */
export function devicePlatformKey(platform: string): string {
  return KNOWN_PLATFORMS.has(platform) ? `devices.platform.${platform}` : 'devices.platform.other';
}

/** The `setup.defaultName` key for this desktop, read from the user agent. */
export function defaultDeviceKey(): 'macos' | 'windows' | 'linux' {
  const ua = navigator.userAgent;
  if (/Windows/i.test(ua)) return 'windows';
  if (/Linux|X11/i.test(ua) && !/Mac/i.test(ua)) return 'linux';
  return 'macos';
}

/** A device not heard from for more days than this is marked as quiet in the device list. */
export const QUIET_AFTER_DAYS = 30;

const SECONDS_PER_DAY = 86_400;

/**
 * Whole days since a device was last heard from (both unix seconds), when that is more than
 * QUIET_AFTER_DAYS; null otherwise or when unknown.
 */
export function quietDays(lastActiveAt: number | null, now: number): number | null {
  if (lastActiveAt === null) return null;
  const days = Math.floor((now - lastActiveAt) / SECONDS_PER_DAY);
  return days > QUIET_AFTER_DAYS ? days : null;
}
