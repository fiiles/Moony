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
