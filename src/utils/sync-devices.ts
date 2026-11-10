const KNOWN_PLATFORMS = new Set(['macos', 'windows', 'linux', 'ios', 'android']);

/** The i18n key of a device's platform label; unknown platforms read as "other". */
export function devicePlatformKey(platform: string): string {
  return KNOWN_PLATFORMS.has(platform) ? `devices.platform.${platform}` : 'devices.platform.other';
}
